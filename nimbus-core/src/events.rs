use crate::types::{
    AdapterCapabilities, BandwidthSample, DashboardStats, HotspotInfo, HotspotState,
    NetworkInterface, Page, ScannedNetwork, StationInfo,
};

#[derive(Debug, Clone)]
pub enum UiEvent {
    HotspotStateChanged(HotspotState),
    StationsUpdated(Vec<StationInfo>),
    StatsUpdated(DashboardStats),
    AdapterInfo(AdapterCapabilities),
    InterfacesDetected(Vec<NetworkInterface>),
    ScannedNetworks(Vec<ScannedNetwork>),
    ErrorOccurred(String),
    ShowToast {
        message: String,
        kind: ToastKind,
    },
    NavigateTo(Page),
    QrCodeGenerated(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub enum BackendCommand {
    StartHotspot {
        config: crate::types::HotspotConfig,
        interface: String,
    },
    StopHotspot,
    GetStations,
    GetAdapterInfo {
        interface: String,
    },
    ScanNetworks,
    DetectInterfaces,
}

#[derive(Debug, Clone)]
pub enum BackendEvent {
    HotspotStarted(HotspotInfo),
    HotspotStopped,
    HotspotError(String),
    StationsUpdated(Vec<StationInfo>),
    AdapterInfo(AdapterCapabilities),
    InterfacesDetected(Vec<NetworkInterface>),
    BandwidthUpdate(BandwidthSample),
}
