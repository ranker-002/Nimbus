use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{watch, Mutex};

use nimbus_core::error::NimbusError;
use nimbus_core::events::{BackendCommand, ToastKind, UiEvent};
use nimbus_core::types::{
    BandwidthSample, DashboardStats, HotspotBackend, HotspotConfig, HotspotInfo, HotspotPlan,
    HotspotState, StationInfo,
};
use nimbus_network::regdomain::RegDomainGuard;
use nimbus_network::traits::NetworkManagerApi;
use nimbus_telemetry::bandwidth::BandwidthTracker;
use nimbus_telemetry::history::HistoryDb;

use crate::firewall::FirewallManager;
use crate::planner::plan_hotspot;
use crate::shared_ap::SharedAp;

/// How often live figures are refreshed while a hotspot is up.
const STATS_INTERVAL: Duration = Duration::from_secs(1);

pub struct Orchestrator {
    nm: Arc<dyn NetworkManagerApi>,
    firewall: FirewallManager,
    state_tx: watch::Sender<HotspotState>,
    event_tx: async_channel::Sender<UiEvent>,
    current_interface: Mutex<Option<String>>,
    started_at: Mutex<Option<Instant>>,
    /// The config the running hotspot was started with, so the poller knows
    /// limits such as `max_clients`.
    active_config: Mutex<Option<HotspotConfig>>,
    /// Remembers the machine's regulatory domain so it can be put back.
    regdomain: Mutex<RegDomainGuard>,
    /// Set while the shared backend is running; owns hostapd, dnsmasq and the
    /// virtual interface.
    shared_ap: Mutex<Option<SharedAp>>,
    /// Guards against stacking up several stats pollers.
    stats_running: AtomicBool,
    /// Local session history. `None` when the database could not be opened;
    /// the rest of the app keeps working without it.
    history: Mutex<Option<HistoryDb>>,
    /// Row id of the session currently being recorded, if any.
    history_record: Mutex<Option<i64>>,
}

impl Orchestrator {
    pub fn new(nm: Arc<dyn NetworkManagerApi>, event_tx: async_channel::Sender<UiEvent>) -> Self {
        let (state_tx, _) = watch::channel(HotspotState::Inactive);
        let history = match HistoryDb::open() {
            Ok(db) => Some(db),
            Err(e) => {
                log::warn!("History unavailable: {}", e);
                None
            }
        };
        Self {
            nm,
            firewall: FirewallManager::new(),
            state_tx,
            event_tx,
            current_interface: Mutex::new(None),
            started_at: Mutex::new(None),
            active_config: Mutex::new(None),
            regdomain: Mutex::new(RegDomainGuard::new()),
            shared_ap: Mutex::new(None),
            stats_running: AtomicBool::new(false),
            history: Mutex::new(history),
            history_record: Mutex::new(None),
        }
    }

    pub fn state_receiver(&self) -> watch::Receiver<HotspotState> {
        self.state_tx.subscribe()
    }

    pub async fn handle_command(self: &Arc<Self>, cmd: BackendCommand) {
        match cmd {
            BackendCommand::StartHotspot {
                config,
                interface,
                confirmed,
            } => self.start_hotspot(config, interface, confirmed).await,
            BackendCommand::StopHotspot => self.stop_hotspot().await,
            BackendCommand::RefreshStatus => self.refresh_status().await,
            BackendCommand::GetStations => self.get_stations().await,
            BackendCommand::DisconnectStation { mac } => self.disconnect_station(mac).await,
            BackendCommand::GetAdapterInfo { interface } => self.get_adapter_info(interface).await,
            BackendCommand::ScanNetworks => self.scan_networks().await,
            BackendCommand::DetectInterfaces => self.detect_interfaces().await,
            BackendCommand::GetHistory { limit } => self.get_history(limit).await,
            BackendCommand::GetRegulatoryDomain => {
                let domain = nimbus_network::regdomain::current().await;
                self.emit(UiEvent::RegulatoryDomain(domain)).await;
            }
        }
    }

    async fn emit(&self, event: UiEvent) {
        if self.event_tx.send(event).await.is_err() {
            log::debug!("UI event dropped: no receiver");
        }
    }

    async fn toast(&self, message: impl Into<String>, kind: ToastKind) {
        self.emit(UiEvent::ShowToast {
            message: message.into(),
            kind,
        })
        .await;
    }

    /// Updates the shared state and tells the UI about it in one step, so the
    /// two can never drift apart.
    async fn set_state(&self, state: HotspotState) {
        let _ = self.state_tx.send(state.clone());
        self.emit(UiEvent::HotspotStateChanged(state)).await;
    }

    async fn fail(&self, message: String) {
        self.set_state(HotspotState::Error(message.clone())).await;
        self.toast(message, ToastKind::Error).await;
    }

    async fn start_hotspot(
        self: &Arc<Self>,
        config: HotspotConfig,
        interface: Option<String>,
        confirmed: bool,
    ) {
        let plan = match plan_hotspot(self.nm.as_ref(), &config, interface.as_deref()).await {
            Ok(plan) => plan,
            Err(e) => return self.fail(e.to_string()).await,
        };

        // Nothing has touched the network yet. If going ahead would drop the
        // user's connection, stop here and let them decide.
        if plan.needs_confirmation() && !confirmed {
            let reason = plan
                .warnings
                .first()
                .cloned()
                .unwrap_or_else(|| "This will disconnect your Wi-Fi.".into());
            return self
                .emit(UiEvent::ConfirmationRequired { plan, reason })
                .await;
        }

        for warning in &plan.warnings {
            log::warn!("{}", warning);
        }
        self.set_state(HotspotState::Starting).await;

        // The regulatory domain decides which channels are legal, so it has to
        // be in place before the AP is created.
        if let Some(change) = &plan.regdomain_change {
            match self.regdomain.lock().await.apply(&change.to).await {
                Ok(true) => log::info!("Regulatory domain set to {}", change.to),
                Ok(false) => {}
                Err(e) => return self.fail(format!("Could not set country: {}", e)).await,
            }
        }

        let (info, live_interface) = match self.bring_up(&plan).await {
            Ok(result) => result,
            Err(e) => {
                // Leave the machine's regulatory domain as we found it.
                let _ = self.regdomain.lock().await.restore().await;
                return self.fail(e.to_string()).await;
            }
        };

        *self.current_interface.lock().await = Some(live_interface.clone());
        *self.started_at.lock().await = Some(Instant::now());
        *self.active_config.lock().await = Some(plan.effective_config.clone());
        self.record_session_start(&info.ssid, &live_interface).await;
        self.set_state(HotspotState::Active(info.ssid.clone()))
            .await;
        self.toast(
            format!("Hotspot '{}' started", info.ssid),
            ToastKind::Success,
        )
        .await;

        for warning in plan.warnings {
            self.toast(warning, ToastKind::Warning).await;
        }

        self.spawn_stats_poller();
    }

    /// Creates the access point using whichever backend the plan selected.
    ///
    /// Returns the hotspot details and the interface it actually landed on —
    /// the shared backend runs on its own virtual interface, not the adapter
    /// named in the plan.
    async fn bring_up(&self, plan: &HotspotPlan) -> nimbus_core::Result<(HotspotInfo, String)> {
        match plan.backend {
            HotspotBackend::SharedVirtualAp => self.bring_up_shared(plan).await,
            HotspotBackend::NetworkManager => {
                let info = self
                    .nm
                    .create_hotspot(&plan.effective_config, &plan.ap_interface)
                    .await?;
                let interface = info.interface.clone();
                Ok((info, interface))
            }
        }
    }

    /// Brings up the access point on its own interface so the machine keeps its
    /// Wi-Fi connection and shares it.
    async fn bring_up_shared(
        &self,
        plan: &HotspotPlan,
    ) -> nimbus_core::Result<(HotspotInfo, String)> {
        let channel = plan
            .channel
            .or(plan.effective_config.channel)
            .ok_or_else(|| {
                NimbusError::ConfigError(
                    "Sharing the connection needs a channel to match your Wi-Fi".into(),
                )
            })?;

        // Client traffic leaves through whatever is carrying the machine's
        // own connection.
        let upstream = plan
            .upstream_interface
            .clone()
            .ok_or(NimbusError::NoUpstreamInterface)?;

        let base_mac = self
            .nm
            .get_wifi_devices()
            .await?
            .into_iter()
            .find(|d| d.name == plan.ap_interface)
            .map(|d| d.mac)
            .ok_or_else(|| NimbusError::InterfaceNotFound(plan.ap_interface.clone()))?;

        let ap = SharedAp::start(
            &plan.effective_config,
            &plan.ap_interface,
            base_mac,
            channel,
            &upstream,
        )
        .await?;

        let info = ap.info(&plan.effective_config, channel);
        let interface = ap.interface().to_string();
        *self.shared_ap.lock().await = Some(ap);
        Ok((info, interface))
    }

    async fn stop_hotspot(&self) {
        self.set_state(HotspotState::Stopping).await;

        // Recover the interface if this process did not start the hotspot
        // (for example the app was restarted while it was running).
        let iface = match self.current_interface.lock().await.clone() {
            Some(iface) => Some(iface),
            None => self
                .nm
                .get_active_hotspot()
                .await
                .ok()
                .flatten()
                .map(|info| info.interface),
        };

        if let Some(iface) = &iface {
            let upstream = self.get_upstream().await.unwrap_or_default();
            if let Err(e) = self.firewall.cleanup(iface, &upstream).await {
                // Not fatal: the hotspot itself still needs to come down.
                log::warn!("Firewall cleanup failed: {}", e);
            }
        }

        // The shared backend owns its own processes and interface.
        if let Some(ap) = self.shared_ap.lock().await.take() {
            ap.shutdown().await;
            self.finish_session(iface.as_deref()).await;
            if let Err(e) = self.regdomain.lock().await.restore().await {
                log::warn!("Could not restore the regulatory domain: {}", e);
            }
            *self.started_at.lock().await = None;
            *self.current_interface.lock().await = None;
            *self.active_config.lock().await = None;
            self.set_state(HotspotState::Inactive).await;
            self.toast("Hotspot stopped", ToastKind::Info).await;
            return;
        }

        match self.nm.stop_hotspot().await {
            Ok(()) => {
                self.finish_session(iface.as_deref()).await;
                if let Err(e) = self.regdomain.lock().await.restore().await {
                    log::warn!("Could not restore the regulatory domain: {}", e);
                }
                *self.started_at.lock().await = None;
                *self.current_interface.lock().await = None;
                *self.active_config.lock().await = None;
                self.set_state(HotspotState::Inactive).await;
                self.toast("Hotspot stopped", ToastKind::Info).await;
            }
            Err(e) => self.fail(e.to_string()).await,
        }
    }

    /// Holds the connected-device count at `max_clients` by disconnecting the
    /// most recently arrived devices.
    ///
    /// Returns the devices that were turned away. NetworkManager has no
    /// station limit for AP mode, so this is enforced here: the poller checks
    /// every second and deauthenticates anything over the limit.
    async fn enforce_client_limit(
        &self,
        interface: &str,
        stations: &[StationInfo],
    ) -> Vec<StationInfo> {
        let Some(limit) = self
            .active_config
            .lock()
            .await
            .as_ref()
            .and_then(|c| c.max_clients)
        else {
            return Vec::new();
        };

        let limit = limit as usize;
        if stations.len() <= limit {
            return Vec::new();
        }

        // Keep the devices that have been connected longest; the newest
        // arrivals are the ones over the limit.
        let mut by_age: Vec<&StationInfo> = stations.iter().collect();
        by_age.sort_by_key(|s| std::cmp::Reverse(s.connected_since));

        let mut removed = Vec::new();
        for station in by_age.into_iter().take(stations.len() - limit) {
            match self.nm.disconnect_station(interface, &station.mac).await {
                Ok(()) => {
                    log::info!(
                        "Disconnected {} — hotspot is limited to {} devices",
                        station.mac,
                        limit
                    );
                    removed.push(station.clone());
                }
                Err(e) => log::warn!("Could not enforce the device limit: {}", e),
            }
        }
        removed
    }

    /// Adopts a Nimbus hotspot that is already running, and clears away any
    /// wreckage from a previous run first.
    async fn refresh_status(self: &Arc<Self>) {
        // Only wreckage if this process is not the one running it.
        if self.shared_ap.lock().await.is_none() && crate::shared_ap::cleanup_orphans().await {
            self.toast(
                "Cleared a leftover hotspot from an earlier run",
                ToastKind::Info,
            )
            .await;
        }
        match self.nm.get_active_hotspot().await {
            Ok(Some(info)) => {
                *self.current_interface.lock().await = Some(info.interface.clone());
                let mut started = self.started_at.lock().await;
                if started.is_none() {
                    *started = Some(info.started_at);
                }
                drop(started);
                self.set_state(HotspotState::Active(info.ssid)).await;
                self.spawn_stats_poller();
            }
            Ok(None) => {
                // Bind before awaiting: the watch guard must not be held
                // across an await point.
                let was_active = !matches!(*self.state_tx.borrow(), HotspotState::Inactive);
                if was_active {
                    *self.current_interface.lock().await = None;
                    *self.started_at.lock().await = None;
                    self.set_state(HotspotState::Inactive).await;
                }
            }
            Err(e) => log::warn!("Could not read hotspot status: {}", e),
        }
    }

    /// Publishes station and bandwidth figures for as long as a hotspot is up.
    fn spawn_stats_poller(self: &Arc<Self>) {
        if self.stats_running.swap(true, Ordering::SeqCst) {
            return; // Already polling.
        }

        let this = Arc::clone(self);
        tokio::spawn(async move {
            let mut tracker: Option<BandwidthTracker> = None;

            loop {
                tokio::time::sleep(STATS_INTERVAL).await;

                let still_active = matches!(*this.state_tx.borrow(), HotspotState::Active(_));
                if !still_active {
                    break;
                }
                let Some(iface) = this.current_interface.lock().await.clone() else {
                    break;
                };

                // Re-create the tracker when the hotspot moves interfaces so
                // rates are not computed across two different counters.
                if tracker.as_ref().is_none_or(|t| t.interface() != iface) {
                    tracker = Some(BandwidthTracker::new(&iface));
                }
                let bandwidth = tracker
                    .as_mut()
                    .map(|t| t.sample())
                    .unwrap_or(BandwidthSample {
                        rx_rate: 0,
                        tx_rate: 0,
                        total_rx: 0,
                        total_tx: 0,
                    });

                let mut stations = this
                    .nm
                    .get_connected_stations(&iface)
                    .await
                    .unwrap_or_default();
                enrich_manufacturers(&mut stations);

                let turned_away = this.enforce_client_limit(&iface, &stations).await;
                if !turned_away.is_empty() {
                    // Report the list without the devices just kicked off, so
                    // the count matches what the limit allows.
                    stations.retain(|s| !turned_away.iter().any(|r| r.mac == s.mac));
                    this.toast(
                        format!(
                            "{} device(s) turned away: the hotspot is limited to {} devices",
                            turned_away.len(),
                            stations.len()
                        ),
                        ToastKind::Warning,
                    )
                    .await;
                }

                let uptime_secs = this
                    .started_at
                    .lock()
                    .await
                    .map(|t| t.elapsed().as_secs())
                    .unwrap_or(0);

                this.emit(UiEvent::StatsUpdated(DashboardStats {
                    connected_stations: stations.len() as u32,
                    bandwidth,
                    uptime_secs,
                }))
                .await;
                this.emit(UiEvent::StationsUpdated(stations)).await;
            }

            this.stats_running.store(false, Ordering::SeqCst);
        });
    }

    async fn get_stations(&self) {
        let Some(iface) = self.current_interface.lock().await.clone() else {
            return;
        };
        match self.nm.get_connected_stations(&iface).await {
            Ok(mut stations) => {
                enrich_manufacturers(&mut stations);
                self.emit(UiEvent::StationsUpdated(stations)).await
            }
            Err(e) => self.emit(UiEvent::ErrorOccurred(e.to_string())).await,
        }
    }

    async fn get_adapter_info(&self, interface: String) {
        match self.nm.get_adapter_capabilities(&interface).await {
            Ok(caps) => self.emit(UiEvent::AdapterInfo(caps)).await,
            Err(e) => self.emit(UiEvent::ErrorOccurred(e.to_string())).await,
        }
    }

    async fn scan_networks(&self) {
        // Scanning needs a Wi-Fi adapter, not necessarily an active hotspot.
        let interface = match self.current_interface.lock().await.clone() {
            Some(iface) => Some(iface),
            None => self
                .nm
                .get_wifi_devices()
                .await
                .ok()
                .and_then(|devices| devices.first().map(|d| d.name.clone())),
        };

        let Some(interface) = interface else {
            return self
                .toast("No Wi-Fi adapter available to scan", ToastKind::Warning)
                .await;
        };

        match nimbus_wifi::scanner::scan_available_networks(&interface).await {
            Ok(networks) => {
                let scanned: Vec<nimbus_core::types::ScannedNetwork> = networks
                    .into_iter()
                    .map(|n| nimbus_core::types::ScannedNetwork {
                        ssid: n.ssid,
                        bssid: n.bssid,
                        frequency: n.frequency,
                        signal_dbm: n.signal_dbm,
                        channel: n.channel,
                    })
                    .collect();
                self.emit(UiEvent::ScannedNetworks(scanned)).await;
            }
            Err(e) => {
                self.toast(format!("Scan failed: {}", e), ToastKind::Error)
                    .await
            }
        }
    }

    async fn detect_interfaces(&self) {
        match self.nm.get_all_interfaces().await {
            Ok(interfaces) => self.emit(UiEvent::InterfacesDetected(interfaces)).await,
            Err(e) => self.emit(UiEvent::ErrorOccurred(e.to_string())).await,
        }
    }

    /// Pages of past sessions, for the dashboard's history list.
    async fn get_history(&self, limit: usize) {
        let records = {
            let history = self.history.lock().await;
            match history.as_ref() {
                Some(db) => db.get_recent(limit).unwrap_or_else(|e| {
                    log::warn!("Could not read history: {}", e);
                    Vec::new()
                }),
                None => Vec::new(),
            }
        };
        self.emit(UiEvent::History(records)).await;
    }

    async fn record_session_start(&self, ssid: &str, interface: &str) {
        let id = {
            let mut history = self.history.lock().await;
            match history.as_mut() {
                Some(db) => match db.start_record(ssid, interface) {
                    Ok(id) => Some(id),
                    Err(e) => {
                        log::warn!("Could not record session start: {}", e);
                        None
                    }
                },
                None => None,
            }
        };
        *self.history_record.lock().await = id;
    }

    /// Closes the current history row, with the bytes the interface moved and
    /// how many devices were attached when it went down.
    async fn finish_session(&self, interface: Option<&str>) {
        let Some(record) = self.history_record.lock().await.take() else {
            return;
        };
        let stations = match interface {
            Some(iface) => self
                .nm
                .get_connected_stations(iface)
                .await
                .map(|s| s.len() as u32)
                .unwrap_or(0),
            None => 0,
        };
        let (rx, tx) = interface.map(interface_bytes).unwrap_or((0, 0));

        let mut history = self.history.lock().await;
        if let Some(db) = history.as_mut() {
            if let Err(e) = db.end_record(record, stations, rx, tx) {
                log::warn!("Could not record session end: {}", e);
            }
        }
    }

    async fn get_upstream(&self) -> Option<String> {
        self.nm.get_upstream_interface().await.ok().flatten()
    }

    /// Drops one connected device on the user's request.
    async fn disconnect_station(&self, mac: mac_address::MacAddress) {
        let Some(iface) = self.current_interface.lock().await.clone() else {
            return self
                .toast("No hotspot is running", ToastKind::Warning)
                .await;
        };

        match self.nm.disconnect_station(&iface, &mac).await {
            Ok(()) => {
                self.toast(format!("Disconnected {}", mac), ToastKind::Success)
                    .await;
                self.get_stations().await;
            }
            Err(e) => {
                self.toast(
                    format!("Could not disconnect {}: {}", mac, e),
                    ToastKind::Error,
                )
                .await
            }
        }
    }
}

/// Fills in the vendor for devices whose MAC prefix is known.
fn enrich_manufacturers(stations: &mut [StationInfo]) {
    for station in stations {
        if station.manufacturer.is_none() {
            station.manufacturer =
                nimbus_telemetry::manufacturer::lookup_manufacturer(&station.mac);
        }
    }
}

/// Total bytes received and transmitted by `interface`, from sysfs.
fn interface_bytes(interface: &str) -> (u64, u64) {
    let read = |counter: &str| {
        std::fs::read_to_string(format!(
            "/sys/class/net/{}/statistics/{}",
            interface, counter
        ))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
    };
    (read("rx_bytes"), read("tx_bytes"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{config, station, FakeNm};

    /// Builds an orchestrator over `nm`, already "running" a hotspot on wlan0
    /// with the given device limit.
    fn running(nm: Arc<FakeNm>, max_clients: Option<u32>) -> Arc<Orchestrator> {
        let (event_tx, _event_rx) = async_channel::unbounded();
        let orch = Arc::new(Orchestrator::new(nm, event_tx));
        let cfg = HotspotConfig {
            max_clients,
            ..config()
        };
        // The locks are uncontended here, so try_lock cannot fail.
        *orch.active_config.try_lock().unwrap() = Some(cfg);
        *orch.current_interface.try_lock().unwrap() = Some("wlan0".into());
        orch
    }

    const A: [u8; 6] = [0, 0, 0, 0, 0, 1];
    const B: [u8; 6] = [0, 0, 0, 0, 0, 2];
    const C: [u8; 6] = [0, 0, 0, 0, 0, 3];

    #[tokio::test]
    async fn no_limit_leaves_every_device_connected() {
        let nm = Arc::new(FakeNm::new());
        let orch = running(nm.clone(), None);

        let stations = vec![station(A, 300), station(B, 200), station(C, 100)];
        let removed = orch.enforce_client_limit("wlan0", &stations).await;

        assert!(removed.is_empty());
        assert!(nm.disconnected_macs().is_empty());
    }

    #[tokio::test]
    async fn devices_within_the_limit_are_left_alone() {
        let nm = Arc::new(FakeNm::new());
        let orch = running(nm.clone(), Some(3));

        let stations = vec![station(A, 300), station(B, 200), station(C, 100)];
        assert!(orch
            .enforce_client_limit("wlan0", &stations)
            .await
            .is_empty());
        assert!(nm.disconnected_macs().is_empty());
    }

    /// The devices that were there first keep their connection; the newest
    /// arrival is the one turned away.
    #[tokio::test]
    async fn the_newest_device_over_the_limit_is_disconnected() {
        let nm = Arc::new(FakeNm::new());
        let orch = running(nm.clone(), Some(2));

        let stations = vec![station(A, 300), station(B, 200), station(C, 100)];
        let removed = orch.enforce_client_limit("wlan0", &stations).await;

        assert_eq!(removed.len(), 1);
        assert_eq!(nm.disconnected_macs(), vec![station(C, 100).mac]);
    }

    #[tokio::test]
    async fn several_devices_over_the_limit_are_disconnected_newest_first() {
        let nm = Arc::new(FakeNm::new());
        let orch = running(nm.clone(), Some(1));

        let stations = vec![station(A, 300), station(B, 200), station(C, 100)];
        let removed = orch.enforce_client_limit("wlan0", &stations).await;

        assert_eq!(removed.len(), 2);
        // Newest first: C joined most recently, then B. A is the survivor.
        assert_eq!(
            nm.disconnected_macs(),
            vec![station(C, 100).mac, station(B, 200).mac]
        );
    }

    #[tokio::test]
    async fn a_failed_disconnect_is_reported_as_not_removed() {
        // Without CAP_NET_ADMIN the deauth fails; the device stays connected
        // and must not be counted as turned away.
        let nm = Arc::new(FakeNm::new().failing_disconnects());
        let orch = running(nm.clone(), Some(1));

        let stations = vec![station(A, 300), station(B, 100)];
        let removed = orch.enforce_client_limit("wlan0", &stations).await;

        assert!(removed.is_empty());
        assert!(nm.disconnected_macs().is_empty());
    }

    #[tokio::test]
    async fn nothing_is_enforced_without_a_running_hotspot() {
        let nm = Arc::new(FakeNm::new());
        let (event_tx, _event_rx) = async_channel::unbounded();
        let orch = Arc::new(Orchestrator::new(nm.clone(), event_tx));

        let stations = vec![station(A, 300), station(B, 100)];
        assert!(orch
            .enforce_client_limit("wlan0", &stations)
            .await
            .is_empty());
        assert!(nm.disconnected_macs().is_empty());
    }
}
