use std::fmt;
use std::net::Ipv4Addr;
use std::time::Instant;

use chrono::{DateTime, Utc};
use mac_address::MacAddress;
use serde::{Deserialize, Serialize};

pub const APP_ID: &str = "com.nimbus.Hotspot";
pub const APP_NAME: &str = "Nimbus Hotspot";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Band {
    Band2_4Ghz,
    Band5Ghz,
    Auto,
}

impl fmt::Display for Band {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Band::Band2_4Ghz => write!(f, "2.4 GHz"),
            Band::Band5Ghz => write!(f, "5 GHz"),
            Band::Auto => write!(f, "Auto"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Security {
    Open,
    Wpa2,
    Wpa3,
    Wpa2Wpa3Transition,
}

impl fmt::Display for Security {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Security::Open => write!(f, "Open"),
            Security::Wpa2 => write!(f, "WPA2"),
            Security::Wpa3 => write!(f, "WPA3"),
            Security::Wpa2Wpa3Transition => write!(f, "WPA2/WPA3"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ipv4Method {
    Shared,
    Nat,
}

impl fmt::Display for Ipv4Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Ipv4Method::Shared => write!(f, "Shared (NM-managed)"),
            Ipv4Method::Nat => write!(f, "NAT (nftables)"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotspotConfig {
    pub ssid: String,
    pub password: String,
    pub band: Band,
    pub channel: Option<u32>,
    pub country_code: String,
    pub security: Security,
    pub hidden: bool,
    pub max_clients: Option<u32>,
    pub client_isolation: bool,
    pub ipv4_method: Ipv4Method,
    pub auto_start: bool,
}

impl Default for HotspotConfig {
    fn default() -> Self {
        Self {
            ssid: "Nimbus-Hotspot".to_string(),
            password: String::new(),
            band: Band::Auto,
            channel: None,
            country_code: "US".to_string(),
            security: Security::Wpa2Wpa3Transition,
            hidden: false,
            max_clients: Some(10),
            client_isolation: false,
            ipv4_method: Ipv4Method::Shared,
            auto_start: false,
        }
    }
}

impl HotspotConfig {
    pub fn validate(&self) -> crate::Result<()> {
        if self.ssid.is_empty() || self.ssid.len() > 32 {
            return Err(crate::NimbusError::InvalidValue(
                "SSID must be 1-32 characters".into(),
            ));
        }
        if self.security != Security::Open && self.password.len() < 8 {
            return Err(crate::NimbusError::PasswordTooShort);
        }
        if self.security == Security::Wpa3
            && !matches!(
                self.security,
                Security::Wpa3 | Security::Wpa2Wpa3Transition
            )
        {
            return Err(crate::NimbusError::InvalidValue(
                "WPA3 requires compatible security mode".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotspotState {
    Inactive,
    Starting,
    Active(String),
    Stopping,
    Error(String),
}

#[derive(Debug, Clone)]
pub struct HotspotInfo {
    pub interface: String,
    pub ssid: String,
    pub ip: Ipv4Addr,
    pub frequency: u32,
    pub channel: u32,
    pub started_at: Instant,
}

impl PartialEq for HotspotInfo {
    fn eq(&self, other: &Self) -> bool {
        self.interface == other.interface && self.ssid == other.ssid
    }
}

impl Eq for HotspotInfo {}

#[derive(Debug, Clone)]
pub struct StationInfo {
    pub mac: MacAddress,
    pub ip: Option<Ipv4Addr>,
    pub hostname: Option<String>,
    pub manufacturer: Option<String>,
    pub signal_dbm: i32,
    pub signal_percent: u8,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_rate_mbps: f64,
    pub tx_rate_mbps: f64,
    pub connected_since: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct AdapterCapabilities {
    pub interface: String,
    pub phy_name: String,
    pub driver: String,
    pub supports_ap: bool,
    pub supports_wpa3: bool,
    pub supports_wifi_6: bool,
    pub supports_wifi_6e: bool,
    pub supports_wifi_7: bool,
    pub supports_simultaneous_sta_ap: bool,
    pub supported_bands: Vec<Band>,
    pub supported_channels_2ghz: Vec<u32>,
    pub supported_channels_5ghz: Vec<u32>,
    pub max_sta: u32,
}

#[derive(Debug, Clone)]
pub struct NetworkInterface {
    pub name: String,
    pub interface_type: InterfaceType,
    pub mac: MacAddress,
    pub state: InterfaceState,
    pub driver: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterfaceType {
    Wifi,
    Ethernet,
    UsbTethering,
    Modem,
    Vpn,
    Bridge,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterfaceState {
    Up,
    Down,
    Unavailable,
    Disconnected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Page {
    Dashboard,
    Hotspot,
    Devices,
    Settings,
    FirstRun,
}

#[derive(Debug, Clone)]
pub struct ConnectionRecord {
    pub id: i64,
    pub hotspot_uuid: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub interface: String,
    pub stations_connected: u32,
    pub total_rx_bytes: u64,
    pub total_tx_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct BandwidthSample {
    pub rx_rate: u64,
    pub tx_rate: u64,
    pub total_rx: u64,
    pub total_tx: u64,
}

#[derive(Debug, Clone)]
pub struct DashboardStats {
    pub connected_stations: u32,
    pub bandwidth: BandwidthSample,
    pub uptime_secs: u64,
}
