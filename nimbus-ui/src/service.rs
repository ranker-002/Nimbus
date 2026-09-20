//! Client for the privileged Nimbus D-Bus service.
//!
//! Starting the shared backend needs root (`hostapd`, `dnsmasq`, `nft`,
//! `iw reg set`), so it cannot run inside the unprivileged GUI process. When
//! the system service is available, hotspot operations are routed to it over
//! the system bus and its PolicyKit action decides who may call them. The GUI
//! keeps its own local backend for everything the user is allowed to do
//! without privileges, and as a fallback when the service is not running.

use futures::StreamExt;

use nimbus_core::events::UiEvent;
use nimbus_core::types::{
    ConnectionRecord, HotspotConfig, HotspotPlan, HotspotState, ScannedNetwork,
};

#[zbus::proxy(
    interface = "com.nimbus.Hotspot",
    default_service = "com.nimbus.Hotspot",
    default_path = "/com/nimbus/Hotspot"
)]
trait NimbusService {
    fn plan_hotspot_json(&self, config_json: &str, interface: &str) -> zbus::Result<String>;

    fn start_hotspot_json(
        &self,
        config_json: &str,
        interface: &str,
        confirmed: bool,
    ) -> zbus::Result<()>;

    fn stop_hotspot(&self) -> zbus::Result<()>;

    fn get_status_json(&self) -> zbus::Result<String>;

    fn scan_networks_json(&self) -> zbus::Result<String>;

    fn disconnect_station(&self, mac: &str) -> zbus::Result<()>;

    fn get_history_json(&self, limit: u32) -> zbus::Result<String>;

    #[zbus(signal)]
    fn state_changed(&self, state_json: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    fn stations_changed(&self, stations_json: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    fn stats_changed(&self, stats_json: &str) -> zbus::Result<()>;
}

pub struct SystemService {
    proxy: NimbusServiceProxy<'static>,
}

impl SystemService {
    /// Connects to the system bus and checks that the service actually owns
    /// its name. Returns `None` when the service is not installed or not
    /// running, in which case the caller keeps using its local backend.
    pub async fn connect() -> Option<Self> {
        let connection = zbus::Connection::system().await.ok()?;
        let dbus = zbus::fdo::DBusProxy::new(&connection).await.ok()?;
        let name = zbus::names::BusName::try_from("com.nimbus.Hotspot").ok()?;
        if !dbus.name_has_owner(name).await.unwrap_or(false) {
            return None;
        }
        let proxy = NimbusServiceProxy::new(&connection).await.ok()?;
        Some(Self { proxy })
    }

    pub async fn plan(
        &self,
        config: &HotspotConfig,
        interface: Option<&str>,
    ) -> Result<HotspotPlan, String> {
        let json = serde_json::to_string(config).map_err(|e| e.to_string())?;
        let reply = self
            .proxy
            .plan_hotspot_json(&json, interface.unwrap_or(""))
            .await
            .map_err(|e| e.to_string())?;
        serde_json::from_str(&reply).map_err(|e| e.to_string())
    }

    pub async fn start(
        &self,
        config: &HotspotConfig,
        interface: Option<&str>,
    ) -> Result<(), String> {
        let json = serde_json::to_string(config).map_err(|e| e.to_string())?;
        self.proxy
            .start_hotspot_json(&json, interface.unwrap_or(""), true)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn stop(&self) -> Result<(), String> {
        self.proxy.stop_hotspot().await.map_err(|e| e.to_string())
    }

    pub async fn status(&self) -> Result<HotspotState, String> {
        let reply = self
            .proxy
            .get_status_json()
            .await
            .map_err(|e| e.to_string())?;
        serde_json::from_str(&reply).map_err(|e| e.to_string())
    }

    pub async fn scan(&self) -> Result<Vec<ScannedNetwork>, String> {
        let reply = self
            .proxy
            .scan_networks_json()
            .await
            .map_err(|e| e.to_string())?;
        serde_json::from_str(&reply).map_err(|e| e.to_string())
    }

    pub async fn disconnect(&self, mac: mac_address::MacAddress) -> Result<(), String> {
        self.proxy
            .disconnect_station(&mac.to_string())
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn history(&self, limit: u32) -> Result<Vec<ConnectionRecord>, String> {
        let reply = self
            .proxy
            .get_history_json(limit)
            .await
            .map_err(|e| e.to_string())?;
        serde_json::from_str(&reply).map_err(|e| e.to_string())
    }

    /// Forwards the service's signals to the UI event channel.
    pub async fn forward_events(
        &self,
        event_tx: async_channel::Sender<UiEvent>,
    ) -> zbus::Result<()> {
        let mut states = self.proxy.receive_state_changed().await?;
        let mut stations = self.proxy.receive_stations_changed().await?;
        let mut stats = self.proxy.receive_stats_changed().await?;

        {
            let event_tx = event_tx.clone();
            tokio::spawn(async move {
                while let Some(signal) = states.next().await {
                    let Ok(args) = signal.args() else { continue };
                    if let Ok(state) = serde_json::from_str::<HotspotState>(args.state_json) {
                        let _ = event_tx.send(UiEvent::HotspotStateChanged(state)).await;
                    }
                }
            });
        }
        {
            let event_tx = event_tx.clone();
            tokio::spawn(async move {
                while let Some(signal) = stations.next().await {
                    let Ok(args) = signal.args() else { continue };
                    if let Ok(list) = serde_json::from_str::<Vec<nimbus_core::types::StationInfo>>(
                        args.stations_json,
                    ) {
                        let _ = event_tx.send(UiEvent::StationsUpdated(list)).await;
                    }
                }
            });
        }
        {
            let event_tx = event_tx.clone();
            tokio::spawn(async move {
                while let Some(signal) = stats.next().await {
                    let Ok(args) = signal.args() else { continue };
                    if let Ok(stats) =
                        serde_json::from_str::<nimbus_core::types::DashboardStats>(args.stats_json)
                    {
                        let _ = event_tx.send(UiEvent::StatsUpdated(stats)).await;
                    }
                }
            });
        }

        Ok(())
    }
}
