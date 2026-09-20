use std::sync::Arc;
use std::time::Duration;

use zbus::connection::Builder;
use zbus::interface;

use nimbus_backend::orchestrator::Orchestrator;
use nimbus_backend::planner::plan_hotspot;
use nimbus_core::events::{BackendCommand, UiEvent};
use nimbus_core::types::{Band, HotspotConfig, HotspotState, Security};
use nimbus_network::manager::NmManager;
use nimbus_network::traits::NetworkManagerApi;

const START_TIMEOUT: Duration = Duration::from_secs(120);
const STOP_TIMEOUT: Duration = Duration::from_secs(30);

struct NimbusDbus {
    nm: Arc<NmManager>,
    orchestrator: Arc<Orchestrator>,
}

impl NimbusDbus {
    fn config(ssid: &str, password: &str, band: &str) -> HotspotConfig {
        HotspotConfig {
            ssid: ssid.to_string(),
            password: password.to_string(),
            band: match band {
                "2.4" => Band::Band2_4Ghz,
                "5" => Band::Band5Ghz,
                _ => Band::Auto,
            },
            security: Security::Wpa2Wpa3Transition,
            ..Default::default()
        }
    }

    /// Waits for the orchestrator to reach a state matching `wanted`.
    async fn await_state<F>(&self, wanted: F) -> Result<HotspotState, String>
    where
        F: Fn(&HotspotState) -> bool,
    {
        let mut rx = self.orchestrator.state_receiver();
        let current = rx.borrow().clone();
        if wanted(&current) {
            return Ok(current);
        }

        let deadline = tokio::time::Instant::now() + START_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err("timed out waiting for the hotspot".into());
            }
            match tokio::time::timeout(remaining, rx.changed()).await {
                Ok(Ok(())) => {
                    let state = rx.borrow().clone();
                    if wanted(&state) {
                        return Ok(state);
                    }
                }
                Ok(Err(_)) => return Err("backend stopped".into()),
                Err(_) => return Err("timed out waiting for the hotspot".into()),
            }
        }
    }
}

#[interface(name = "com.nimbus.Hotspot")]
impl NimbusDbus {
    /// Starts a hotspot. Refuses if doing so would disconnect this machine's
    /// own Wi-Fi, unless `force` is set — callers should show the description
    /// from `PlanHotspot` before passing `true`.
    async fn start_hotspot(
        &self,
        ssid: &str,
        password: &str,
        band: &str,
        force: bool,
    ) -> zbus::fdo::Result<String> {
        let config = Self::config(ssid, password, band);

        let plan = plan_hotspot(self.nm.as_ref(), &config, None)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;

        if plan.needs_confirmation() && !force {
            return Err(zbus::fdo::Error::Failed(format!(
                "Refused: {} Pass force=true to override.",
                plan.warnings
                    .first()
                    .map(String::as_str)
                    .unwrap_or("this would disconnect your Wi-Fi.")
            )));
        }

        self.orchestrator
            .handle_command(BackendCommand::StartHotspot {
                config,
                interface: None,
                confirmed: true,
            })
            .await;

        match self
            .await_state(|state| matches!(state, HotspotState::Active(_) | HotspotState::Error(_)))
            .await
        {
            Ok(HotspotState::Active(ssid)) => Ok(format!("Started '{}'", ssid)),
            Ok(HotspotState::Error(message)) => Err(zbus::fdo::Error::Failed(message)),
            Ok(_) => Err(zbus::fdo::Error::Failed("unexpected hotspot state".into())),
            Err(e) => Err(zbus::fdo::Error::Failed(e)),
        }
    }

    /// Describes what `StartHotspot` would do, without changing anything.
    async fn plan_hotspot(
        &self,
        ssid: &str,
        password: &str,
        band: &str,
    ) -> zbus::fdo::Result<String> {
        let config = Self::config(ssid, password, band);
        let plan = plan_hotspot(self.nm.as_ref(), &config, None)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;

        let mut description = format!(
            "adapter={} backend={} upstream={} disconnects={}",
            plan.ap_interface,
            plan.backend,
            plan.upstream_interface.as_deref().unwrap_or("none"),
            plan.needs_confirmation()
        );
        for warning in &plan.warnings {
            description.push('\n');
            description.push_str(warning);
        }
        Ok(description)
    }

    async fn stop_hotspot(&self) -> zbus::fdo::Result<String> {
        self.orchestrator
            .handle_command(BackendCommand::StopHotspot)
            .await;

        let deadline = tokio::time::Instant::now() + STOP_TIMEOUT;
        loop {
            let state = self.orchestrator.state_receiver().borrow().clone();
            match state {
                HotspotState::Inactive => return Ok("Stopped".into()),
                HotspotState::Error(message) => return Err(zbus::fdo::Error::Failed(message)),
                _ if tokio::time::Instant::now() >= deadline => {
                    return Err(zbus::fdo::Error::Failed("timed out stopping".into()))
                }
                _ => tokio::time::sleep(Duration::from_millis(200)).await,
            }
        }
    }

    async fn get_status(&self) -> zbus::fdo::Result<String> {
        let state = self.orchestrator.state_receiver().borrow().clone();
        Ok(match state {
            HotspotState::Inactive => "Inactive".into(),
            HotspotState::Starting => "Starting".into(),
            HotspotState::Active(ssid) => format!("Active: {}", ssid),
            HotspotState::Stopping => "Stopping".into(),
            HotspotState::Error(message) => format!("Error: {}", message),
        })
    }

    async fn get_interfaces(&self) -> zbus::fdo::Result<Vec<String>> {
        match self.nm.get_all_interfaces().await {
            Ok(interfaces) => {
                let names: Vec<String> = interfaces.iter().map(|i| i.name.clone()).collect();
                Ok(names)
            }
            Err(e) => Err(zbus::fdo::Error::Failed(e.to_string())),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let nm = Arc::new(NmManager::new().await);
    let (event_tx, event_rx) = async_channel::bounded::<UiEvent>(256);
    let orchestrator = Arc::new(Orchestrator::new(nm.clone(), event_tx));

    // The orchestrator blocks when nobody drains its events; this service does
    // not show them to anyone, so keep the queue moving and log the important
    // ones.
    tokio::spawn(async move {
        while let Ok(event) = event_rx.recv().await {
            match event {
                UiEvent::ErrorOccurred(message) => log::error!("{}", message),
                UiEvent::HotspotStateChanged(state) => log::info!("State: {:?}", state),
                _ => {}
            }
        }
    });

    let dbus = NimbusDbus { nm, orchestrator };

    let _conn = Builder::system()?
        .name("com.nimbus.Hotspot")?
        .serve_at("/com/nimbus/Hotspot", dbus)?
        .build()
        .await?;

    log::info!("Nimbus D-Bus service running. Press Ctrl+C to exit.");

    std::future::pending::<()>().await;

    Ok(())
}
