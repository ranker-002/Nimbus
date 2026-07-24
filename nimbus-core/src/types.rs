use std::fmt;
use std::net::Ipv4Addr;
use std::time::Instant;

use chrono::{DateTime, Utc};
use mac_address::MacAddress;
use serde::{Deserialize, Serialize};

use crate::constants::{MAX_CLIENTS_MAX, MAX_PASSWORD_LEN, MAX_SSID_LEN, MIN_PASSWORD_LEN};

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
        if self.ssid.is_empty() || self.ssid.len() > MAX_SSID_LEN {
            return Err(crate::NimbusError::InvalidValue(format!(
                "SSID must be 1-{} characters",
                MAX_SSID_LEN
            )));
        }

        if self.security != Security::Open {
            if self.password.len() < MIN_PASSWORD_LEN {
                return Err(crate::NimbusError::PasswordTooShort);
            }
            if self.password.len() > MAX_PASSWORD_LEN {
                return Err(crate::NimbusError::InvalidValue(format!(
                    "Password must be at most {} characters",
                    MAX_PASSWORD_LEN
                )));
            }
        }

        if let Some(max) = self.max_clients {
            if max > MAX_CLIENTS_MAX {
                return Err(crate::NimbusError::InvalidValue(format!(
                    "Max clients must be at most {}",
                    MAX_CLIENTS_MAX
                )));
            }
        }

        if let Some(ch) = self.channel {
            match self.band {
                Band::Band2_4Ghz => {
                    if !(1..=13).contains(&ch) {
                        return Err(crate::NimbusError::InvalidValue(
                            "Channel for 2.4 GHz must be 1-13".into(),
                        ));
                    }
                }
                Band::Band5Ghz => {
                    if !(36..=165).contains(&ch) || ch % 4 != 0 {
                        return Err(crate::NimbusError::InvalidValue(
                            "Channel for 5 GHz must be 36-165 (step 4)".into(),
                        ));
                    }
                }
                Band::Auto => {}
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[derive(Default)]
pub enum HotspotState {
    #[default]
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
        self.interface == other.interface
            && self.ssid == other.ssid
            && self.ip == other.ip
            && self.frequency == other.frequency
            && self.channel == other.channel
    }
}

impl Eq for HotspotInfo {}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone)]
pub struct ScannedNetwork {
    pub ssid: String,
    pub bssid: String,
    pub frequency: u32,
    pub signal_dbm: i32,
    pub channel: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_config() -> HotspotConfig {
        HotspotConfig {
            ssid: "TestNetwork".to_string(),
            password: "password123".to_string(),
            band: Band::Auto,
            channel: None,
            country_code: "US".to_string(),
            security: Security::Wpa2,
            hidden: false,
            max_clients: Some(10),
            client_isolation: false,
            ipv4_method: Ipv4Method::Shared,
            auto_start: false,
        }
    }

    #[test]
    fn test_valid_config_passes_validation() {
        let config = valid_config();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_empty_ssid_fails() {
        let mut config = valid_config();
        config.ssid = String::new();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_ssid_too_long_fails() {
        let mut config = valid_config();
        config.ssid = "A".repeat(MAX_SSID_LEN + 1);
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_ssid_max_length_passes() {
        let mut config = valid_config();
        config.ssid = "A".repeat(MAX_SSID_LEN);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_password_too_short_fails() {
        let mut config = valid_config();
        config.password = "short".to_string();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_password_too_long_fails() {
        let mut config = valid_config();
        config.password = "A".repeat(MAX_PASSWORD_LEN + 1);
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_password_min_length_passes() {
        let mut config = valid_config();
        config.password = "A".repeat(MIN_PASSWORD_LEN);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_password_max_length_passes() {
        let mut config = valid_config();
        config.password = "A".repeat(MAX_PASSWORD_LEN);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_open_network_no_password_required() {
        let mut config = valid_config();
        config.security = Security::Open;
        config.password = String::new();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_max_clients_exceeded_fails() {
        let mut config = valid_config();
        config.max_clients = Some(MAX_CLIENTS_MAX + 1);
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_max_clients_at_limit_passes() {
        let mut config = valid_config();
        config.max_clients = Some(MAX_CLIENTS_MAX);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_invalid_channel_2_4ghz_fails() {
        let mut config = valid_config();
        config.band = Band::Band2_4Ghz;
        config.channel = Some(14);
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_valid_channel_2_4ghz_passes() {
        let mut config = valid_config();
        config.band = Band::Band2_4Ghz;
        config.channel = Some(6);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_invalid_channel_5ghz_fails() {
        let mut config = valid_config();
        config.band = Band::Band5Ghz;
        config.channel = Some(37);
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_valid_channel_5ghz_passes() {
        let mut config = valid_config();
        config.band = Band::Band5Ghz;
        config.channel = Some(36);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_auto_band_any_channel_passes() {
        let mut config = valid_config();
        config.band = Band::Auto;
        config.channel = Some(100);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_equality() {
        let config1 = valid_config();
        let config2 = valid_config();
        assert_eq!(config1, config2);
    }

    #[test]
    fn test_config_clone() {
        let config1 = valid_config();
        let config2 = config1.clone();
        assert_eq!(config1, config2);
    }

    #[test]
    fn test_hotspot_state_default() {
        assert_eq!(HotspotState::default(), HotspotState::Inactive);
    }

    #[test]
    fn test_hotspot_state_equality() {
        assert_eq!(HotspotState::Inactive, HotspotState::Inactive);
        assert_eq!(HotspotState::Active("test".into()), HotspotState::Active("test".into()));
        assert_ne!(HotspotState::Inactive, HotspotState::Active("test".into()));
    }

    #[test]
    fn test_band_display() {
        assert_eq!(Band::Band2_4Ghz.to_string(), "2.4 GHz");
        assert_eq!(Band::Band5Ghz.to_string(), "5 GHz");
        assert_eq!(Band::Auto.to_string(), "Auto");
    }

    #[test]
    fn test_security_display() {
        assert_eq!(Security::Open.to_string(), "Open");
        assert_eq!(Security::Wpa2.to_string(), "WPA2");
        assert_eq!(Security::Wpa3.to_string(), "WPA3");
        assert_eq!(Security::Wpa2Wpa3Transition.to_string(), "WPA2/WPA3");
    }
}
