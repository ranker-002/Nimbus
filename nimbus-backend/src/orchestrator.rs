use std::sync::Arc;

use tokio::sync::{mpsc, watch, Mutex};

use nimbus_core::events::{BackendCommand, ToastKind, UiEvent};
use nimbus_core::types::{
    BandwidthSample, DashboardStats, HotspotState,
};
use nimbus_network::traits::NetworkManagerApi;

use crate::firewall::FirewallManager;

pub struct Orchestrator {
    nm: Arc<dyn NetworkManagerApi>,
    firewall: FirewallManager,
    state_tx: watch::Sender<HotspotState>,
    event_tx: mpsc::Sender<UiEvent>,
    current_interface: Mutex<Option<String>>,
}

impl Orchestrator {
    pub fn new(
        nm: Arc<dyn NetworkManagerApi>,
        event_tx: mpsc::Sender<UiEvent>,
    ) -> Self {
        let (state_tx, _) = watch::channel(HotspotState::Inactive);
        Self {
            nm,
            firewall: FirewallManager::new(),
            state_tx,
            event_tx,
            current_interface: Mutex::new(None),
        }
    }

    pub fn state_receiver(&self) -> watch::Receiver<HotspotState> {
        self.state_tx.subscribe()
    }

    pub async fn handle_command(&self, cmd: BackendCommand) {
        match cmd {
            BackendCommand::StartHotspot { config, interface } => {
                self.start_hotspot(config, interface).await
            }
            BackendCommand::StopHotspot => self.stop_hotspot().await,
            BackendCommand::GetStations => self.get_stations().await,
            BackendCommand::GetAdapterInfo { interface } => {
                self.get_adapter_info(interface).await
            }
            BackendCommand::ScanNetworks => self.scan_networks().await,
            BackendCommand::DetectInterfaces => self.detect_interfaces().await,
        }
    }

    async fn start_hotspot(
        &self,
        config: nimbus_core::types::HotspotConfig,
        interface: String,
    ) {
        let _ = self.state_tx.send(HotspotState::Starting);

        match self.nm.create_hotspot(&config, &interface).await {
            Ok(info) => {
                if let Err(e) = self
                    .firewall
                    .setup_nat(&interface, &self.get_upstream().await)
                    .await
                {
                    let _ = self
                        .event_tx
                        .send(UiEvent::ShowToast {
                            message: format!("Firewall warning: {}", e),
                            kind: ToastKind::Warning,
                        })
                        .await;
                }

                let _ = self
                    .state_tx
                    .send(HotspotState::Active(info.ssid.clone()));
                let _ = self
                    .event_tx
                    .send(UiEvent::ShowToast {
                        message: format!("Hotspot '{}' started", config.ssid),
                        kind: ToastKind::Success,
                    })
                    .await;

                *self.current_interface.lock().await = Some(interface);
            }
            Err(e) => {
                let _ = self
                    .state_tx
                    .send(HotspotState::Error(e.to_string()));
                let _ = self
                    .event_tx
                    .send(UiEvent::ShowToast {
                        message: e.to_string(),
                        kind: ToastKind::Error,
                    })
                    .await;
            }
        }
    }

    async fn stop_hotspot(&self) {
        let _ = self.state_tx.send(HotspotState::Stopping);

        if let Some(iface) = self.current_interface.lock().await.as_ref() {
            let upstream = self.get_upstream().await;
            let _ = self.firewall.cleanup(iface, &upstream).await;
        }

        match self.nm.stop_hotspot().await {
            Ok(()) => {
                let _ = self.state_tx.send(HotspotState::Inactive);
                let _ = self
                    .event_tx
                    .send(UiEvent::ShowToast {
                        message: "Hotspot stopped".into(),
                        kind: ToastKind::Info,
                    })
                    .await;
                *self.current_interface.lock().await = None;
            }
            Err(e) => {
                let _ = self
                    .state_tx
                    .send(HotspotState::Error(e.to_string()));
                let _ = self
                    .event_tx
                    .send(UiEvent::ShowToast {
                        message: e.to_string(),
                        kind: ToastKind::Error,
                    })
                    .await;
            }
        }
    }

    async fn get_stations(&self) {
        if let Some(iface) = self.current_interface.lock().await.as_ref() {
            match self.nm.get_connected_stations(iface).await {
                Ok(stations) => {
                    let _ = self
                        .event_tx
                        .send(UiEvent::StationsUpdated(stations))
                        .await;
                }
                Err(e) => {
                    let _ = self
                        .event_tx
                        .send(UiEvent::ErrorOccurred(e.to_string()))
                        .await;
                }
            }
        }
    }

    async fn get_adapter_info(&self, interface: String) {
        match self.nm.get_adapter_capabilities(&interface).await {
            Ok(caps) => {
                let _ = self.event_tx.send(UiEvent::AdapterInfo(caps)).await;
            }
            Err(e) => {
                let _ = self
                    .event_tx
                    .send(UiEvent::ErrorOccurred(e.to_string()))
                    .await;
            }
        }
    }

    async fn scan_networks(&self) {
        let _ = self
            .event_tx
            .send(UiEvent::ShowToast {
                message: "Scanning...".into(),
                kind: ToastKind::Info,
            })
            .await;
    }

    async fn detect_interfaces(&self) {
        match self.nm.get_all_interfaces().await {
            Ok(interfaces) => {
                let _ = self
                    .event_tx
                    .send(UiEvent::InterfacesDetected(interfaces))
                    .await;
            }
            Err(e) => {
                let _ = self
                    .event_tx
                    .send(UiEvent::ErrorOccurred(e.to_string()))
                    .await;
            }
        }
    }

    async fn get_upstream(&self) -> String {
        self.nm
            .get_upstream_interface()
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| "eth0".to_string())
    }

    pub async fn collect_stats(&self) -> Option<DashboardStats> {
        let state = self.state_tx.borrow().clone();
        if let HotspotState::Active(_) = state {
            let interface = self.current_interface.lock().await.clone()?;
            let stations = self
                .nm
                .get_connected_stations(&interface)
                .await
                .unwrap_or_default();

            let bandwidth = self
                .nm
                .get_bandwidth_sample(&interface)
                .await
                .ok()
                .unwrap_or(BandwidthSample {
                    rx_rate: 0,
                    tx_rate: 0,
                    total_rx: 0,
                    total_tx: 0,
                });

            return Some(DashboardStats {
                connected_stations: stations.len() as u32,
                bandwidth,
                uptime_secs: 0,
            });
        }
        None
    }
}
