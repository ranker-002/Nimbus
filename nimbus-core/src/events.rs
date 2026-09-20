use crate::types::{
    AdapterCapabilities, ConnectionRecord, DashboardStats, HotspotConfig, HotspotPlan,
    HotspotState, NetworkInterface, ScannedNetwork, StationInfo,
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
    /// Past hotspot sessions, newest first.
    History(Vec<ConnectionRecord>),
    /// The machine's current wireless regulatory domain, or `None` if it could
    /// not be determined.
    RegulatoryDomain(Option<String>),
    /// The requested hotspot cannot be brought up without dropping the current
    /// uplink. Nothing has been changed; re-send `StartHotspot` with
    /// `confirmed: true` to go ahead anyway.
    ConfirmationRequired {
        plan: HotspotPlan,
        reason: String,
    },
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
    /// Bring up a hotspot. `interface: None` lets the backend pick the adapter
    /// least likely to cost the user their connection. `confirmed` must be set
    /// by the caller before the backend will do anything that drops the uplink.
    StartHotspot {
        config: HotspotConfig,
        interface: Option<String>,
        confirmed: bool,
    },
    StopHotspot,
    /// Re-read the live NetworkManager state and adopt a Nimbus hotspot that is
    /// already running (e.g. left over from a previous run). Read-only.
    RefreshStatus,
    GetStations,
    /// Drop one device from the running hotspot.
    DisconnectStation {
        mac: mac_address::MacAddress,
    },
    GetAdapterInfo {
        interface: String,
    },
    ScanNetworks,
    DetectInterfaces,
    /// Read past hotspot sessions from the local history database.
    GetHistory {
        limit: usize,
    },
    /// Read the machine's current regulatory domain. Read-only.
    GetRegulatoryDomain,
}
