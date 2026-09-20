use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use zbus::connection::Builder;
use zbus::interface;
use zbus::message::Header;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::Value;

use nimbus_backend::orchestrator::Orchestrator;
use nimbus_backend::planner::plan_hotspot;
use nimbus_core::events::{BackendCommand, UiEvent};
use nimbus_core::types::{HotspotConfig, HotspotState};
use nimbus_network::manager::{parse_mac, validate_mac, NmManager};
use nimbus_network::traits::NetworkManagerApi;
use nimbus_telemetry::history::HistoryDb;

const PATH: &str = "/com/nimbus/Hotspot";
const NAME: &str = "com.nimbus.Hotspot";
const POLKIT_ACTION: &str = "com.nimbus.hotspot.manage";
const START_TIMEOUT: Duration = Duration::from_secs(120);
const STOP_TIMEOUT: Duration = Duration::from_secs(30);

struct NimbusDbus {
    nm: Arc<NmManager>,
    orchestrator: Arc<Orchestrator>,
}

impl NimbusDbus {
    /// Waits for the orchestrator to reach a state matching `wanted`.
    async fn await_state<F>(&self, wanted: F, timeout: Duration) -> Result<HotspotState, String>
    where
        F: Fn(&HotspotState) -> bool,
    {
        let mut rx = self.orchestrator.state_receiver();
        let current = rx.borrow().clone();
        if wanted(&current) {
            return Ok(current);
        }

        let deadline = tokio::time::Instant::now() + timeout;
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

    fn current_state(&self) -> HotspotState {
        self.orchestrator.state_receiver().borrow().clone()
    }

    fn running(&self) -> bool {
        matches!(
            self.current_state(),
            HotspotState::Starting | HotspotState::Active(_) | HotspotState::Stopping
        )
    }
}

fn failed(error: impl std::fmt::Display) -> zbus::fdo::Error {
    zbus::fdo::Error::Failed(error.to_string())
}

/// D-Bus has no nullable strings, so an empty interface name means "let the
/// backend choose".
fn optional_interface(interface: &str) -> Option<&str> {
    (!interface.is_empty()).then_some(interface)
}

/// Asks PolicyKit whether the caller may manage hotspots.
///
/// The action is configured with `allow_active=yes`, so a user on an active
/// local session is authorised without a prompt while inactive or remote
/// sessions need an administrator.
async fn authorize(connection: &zbus::Connection, header: &Header<'_>) -> zbus::fdo::Result<()> {
    let Some(sender) = header.sender() else {
        return Err(zbus::fdo::Error::AuthFailed(
            "the call has no sender to authorize".into(),
        ));
    };

    let proxy = zbus::Proxy::new(
        connection,
        "org.freedesktop.PolicyKit1",
        "/org/freedesktop/PolicyKit1/Authority",
        "org.freedesktop.PolicyKit1.Authority",
    )
    .await
    .map_err(|e| zbus::fdo::Error::AuthFailed(format!("PolicyKit is unavailable: {}", e)))?;

    let mut subject = HashMap::new();
    subject.insert("name", Value::from(sender.as_str()));
    let details: HashMap<&str, &str> = HashMap::new();

    let (authorized, _challenge): (bool, bool) = proxy
        .call(
            "CheckAuthorization",
            &(
                ("system-bus-name", subject),
                POLKIT_ACTION,
                details,
                1u32,
                "",
            ),
        )
        .await
        .map_err(|e| zbus::fdo::Error::AuthFailed(format!("PolicyKit check failed: {}", e)))?;

    if authorized {
        Ok(())
    } else {
        Err(zbus::fdo::Error::AuthFailed(
            "Not authorized to manage Wi-Fi hotspots".into(),
        ))
    }
}

#[interface(name = "com.nimbus.Hotspot")]
impl NimbusDbus {
    /// Work out what starting would do, without changing anything. Returns a
    /// serialised [`nimbus_core::types::HotspotPlan`].
    async fn plan_hotspot_json(
        &self,
        config_json: &str,
        interface: String,
    ) -> zbus::fdo::Result<String> {
        let config: HotspotConfig =
            serde_json::from_str(config_json).map_err(|e| failed(format!("bad config: {}", e)))?;

        let interface = optional_interface(&interface);
        let plan = plan_hotspot(self.nm.as_ref(), &config, interface)
            .await
            .map_err(failed)?;
        serde_json::to_string(&plan).map_err(failed)
    }

    /// Starts a hotspot. Callers must first read the plan and pass
    /// `confirmed = true` if it would disconnect the machine's uplink.
    async fn start_hotspot_json(
        &self,
        #[zbus(connection)] connection: &zbus::Connection,
        #[zbus(header)] header: Header<'_>,
        config_json: &str,
        interface: String,
        confirmed: bool,
    ) -> zbus::fdo::Result<()> {
        authorize(connection, &header).await?;

        let config: HotspotConfig =
            serde_json::from_str(config_json).map_err(|e| failed(format!("bad config: {}", e)))?;

        let interface = optional_interface(&interface);
        let plan = plan_hotspot(self.nm.as_ref(), &config, interface)
            .await
            .map_err(failed)?;

        if plan.needs_confirmation() && !confirmed {
            return Err(failed(format!(
                "confirmation required: {}",
                plan.warnings
                    .first()
                    .map(String::as_str)
                    .unwrap_or("this would disconnect your Wi-Fi")
            )));
        }

        self.orchestrator
            .handle_command(BackendCommand::StartHotspot {
                config,
                interface: interface.map(str::to_string),
                confirmed: true,
            })
            .await;

        match self
            .await_state(
                |state| matches!(state, HotspotState::Active(_) | HotspotState::Error(_)),
                START_TIMEOUT,
            )
            .await
        {
            Ok(HotspotState::Active(_)) => Ok(()),
            Ok(HotspotState::Error(message)) => Err(failed(message)),
            Ok(_) => Err(failed("unexpected hotspot state")),
            Err(e) => Err(failed(e)),
        }
    }

    /// Stops the running hotspot. Idempotent when nothing is running.
    async fn stop_hotspot(
        &self,
        #[zbus(connection)] connection: &zbus::Connection,
        #[zbus(header)] header: Header<'_>,
    ) -> zbus::fdo::Result<()> {
        authorize(connection, &header).await?;

        if !self.running() {
            return Ok(());
        }

        self.orchestrator
            .handle_command(BackendCommand::StopHotspot)
            .await;

        match self
            .await_state(
                |state| !matches!(state, HotspotState::Stopping),
                STOP_TIMEOUT,
            )
            .await
        {
            Ok(HotspotState::Active(_)) | Ok(HotspotState::Starting) => {
                Err(failed("the hotspot is still running"))
            }
            Ok(HotspotState::Error(message)) => Err(failed(message)),
            Ok(_) | Err(_) => Ok(()),
        }
    }

    /// The current hotspot state, as a serialised
    /// [`nimbus_core::types::HotspotState`].
    async fn get_status_json(&self) -> zbus::fdo::Result<String> {
        serde_json::to_string(&self.current_state()).map_err(failed)
    }

    /// Scans for nearby networks and returns serialised
    /// [`nimbus_core::types::ScannedNetwork`] values. Needs CAP_NET_ADMIN,
    /// which is why it lives here rather than in the unprivileged GUI.
    async fn scan_networks_json(
        &self,
        #[zbus(connection)] connection: &zbus::Connection,
        #[zbus(header)] header: Header<'_>,
    ) -> zbus::fdo::Result<String> {
        authorize(connection, &header).await?;

        let devices = self.nm.get_wifi_devices().await.map_err(failed)?;
        let Some(device) = devices.first() else {
            return Err(failed("no Wi-Fi adapter available to scan"));
        };

        let networks = nimbus_wifi::scanner::scan_available_networks(&device.name)
            .await
            .map_err(failed)?;
        serde_json::to_string(&networks).map_err(failed)
    }

    /// Drops one device from the running hotspot.
    async fn disconnect_station(
        &self,
        #[zbus(connection)] connection: &zbus::Connection,
        #[zbus(header)] header: Header<'_>,
        mac: &str,
    ) -> zbus::fdo::Result<()> {
        authorize(connection, &header).await?;

        if !validate_mac(mac) {
            return Err(zbus::fdo::Error::InvalidArgs(format!(
                "'{}' is not a MAC address",
                mac
            )));
        }
        self.orchestrator
            .handle_command(BackendCommand::DisconnectStation {
                mac: parse_mac(mac),
            })
            .await;
        Ok(())
    }

    /// Past sessions, newest first, as serialised
    /// [`nimbus_core::types::ConnectionRecord`] values.
    async fn get_history_json(&self, limit: u32) -> zbus::fdo::Result<String> {
        let db = HistoryDb::open().map_err(failed)?;
        let records = db.get_recent(limit.min(500) as usize).map_err(failed)?;
        serde_json::to_string(&records).map_err(failed)
    }

    async fn get_interfaces(&self) -> zbus::fdo::Result<Vec<String>> {
        match self.nm.get_all_interfaces().await {
            Ok(interfaces) => Ok(interfaces.iter().map(|i| i.name.clone()).collect()),
            Err(e) => Err(failed(e)),
        }
    }

    #[zbus(signal)]
    async fn state_changed(emitter: &SignalEmitter<'_>, state_json: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn stations_changed(emitter: &SignalEmitter<'_>, stations_json: &str)
        -> zbus::Result<()>;

    #[zbus(signal)]
    async fn stats_changed(emitter: &SignalEmitter<'_>, stats_json: &str) -> zbus::Result<()>;
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let nm = Arc::new(NmManager::new().await);
    let (event_tx, event_rx) = async_channel::bounded::<UiEvent>(256);
    let orchestrator = Arc::new(Orchestrator::new(nm.clone(), event_tx));

    let dbus = NimbusDbus { nm, orchestrator };

    let connection = Builder::system()?
        .name(NAME)?
        .serve_at(PATH, dbus)?
        .build()
        .await?;

    // Forward backend events to subscribers. The GUI keeps its state from
    // these signals, so they must keep flowing even when the interface itself
    // is idle.
    let iface_ref = connection
        .object_server()
        .interface::<_, NimbusDbus>(PATH)
        .await?;
    tokio::spawn(async move {
        while let Ok(event) = event_rx.recv().await {
            match event {
                UiEvent::HotspotStateChanged(state) => {
                    if let Ok(json) = serde_json::to_string(&state) {
                        let emitter = iface_ref.signal_emitter();
                        let _ = NimbusDbus::state_changed(emitter, &json).await;
                    }
                }
                UiEvent::StationsUpdated(stations) => {
                    if let Ok(json) = serde_json::to_string(&stations) {
                        let emitter = iface_ref.signal_emitter();
                        let _ = NimbusDbus::stations_changed(emitter, &json).await;
                    }
                }
                UiEvent::StatsUpdated(stats) => {
                    if let Ok(json) = serde_json::to_string(&stats) {
                        let emitter = iface_ref.signal_emitter();
                        let _ = NimbusDbus::stats_changed(emitter, &json).await;
                    }
                }
                UiEvent::ErrorOccurred(message) => log::error!("{}", message),
                _ => {}
            }
        }
    });

    log::info!("Nimbus D-Bus service running. Press Ctrl+C to exit.");

    std::future::pending::<()>().await;

    Ok(())
}
