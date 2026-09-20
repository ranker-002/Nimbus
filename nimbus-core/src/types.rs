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

impl Band {
    /// The band a channel number belongs to, as used by `iw`/NetworkManager.
    /// 2.4 GHz is 1-14, everything above is treated as 5/6 GHz.
    pub fn of_channel(channel: u32) -> Band {
        if (1..=14).contains(&channel) {
            Band::Band2_4Ghz
        } else {
            Band::Band5Ghz
        }
    }
}

/// Centre frequency in MHz for a channel number.
pub fn channel_to_freq(channel: u32) -> u32 {
    match channel {
        14 => 2484,
        1..=13 => 2407 + channel * 5,
        // 6 GHz channels overlap 5 GHz numbering; callers that need to tell
        // them apart should carry the frequency instead of the channel.
        _ => 5000 + channel * 5,
    }
}

/// Whether a channel is subject to radar detection (DFS).
///
/// Access points on these channels must listen for weather radar before they
/// may transmit, which delays start-up by a minute or more and is only allowed
/// at all under a real regulatory domain — the world domain `00` forbids it.
/// Joining such a channel as a client is unrestricted, so a machine can happily
/// be connected on one while an AP cannot start there.
pub fn is_dfs_channel(channel: u32) -> bool {
    (52..=64).contains(&channel) || (100..=144).contains(&channel)
}

/// Channel number for a centre frequency in MHz. Returns 0 when the frequency
/// falls outside the bands Nimbus knows about.
pub fn freq_to_channel(freq: u32) -> u32 {
    match freq {
        2484 => 14,
        f if (2412..=2472).contains(&f) && (f - 2412) % 5 == 0 => (f - 2407) / 5,
        f if (5170..=5895).contains(&f) => (f - 5000) / 5,
        f if (5955..=7115).contains(&f) => (f - 5950) / 5,
        _ => 0,
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
pub struct HotspotConfig {
    pub ssid: String,
    pub password: String,
    pub band: Band,
    pub channel: Option<u32>,
    /// ISO 3166-1 alpha-2 regulatory domain to switch the machine to, or
    /// `None` to leave the system setting alone.
    ///
    /// The kernel keeps one regulatory domain for the whole machine, so setting
    /// this affects every Wi-Fi interface, not just the hotspot. It therefore
    /// defaults to `None`: a hotspot should not silently re-region someone's
    /// laptop.
    #[serde(default, deserialize_with = "deserialize_country_code")]
    pub country_code: Option<String>,
    pub security: Security,
    pub hidden: bool,
    /// Maximum number of devices allowed to stay connected, or `None` for no
    /// limit. Devices beyond the limit are disconnected.
    pub max_clients: Option<u32>,
    pub client_isolation: bool,
    pub auto_start: bool,
}

/// Accepts both the current `Option<String>` shape and the bare string that
/// earlier versions wrote into saved hotspot files.
fn deserialize_country_code<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum CountryCode {
        Absent,
        Value(String),
    }

    Ok(match CountryCode::deserialize(deserializer)? {
        CountryCode::Value(code) if !code.trim().is_empty() => {
            Some(code.trim().to_ascii_uppercase())
        }
        _ => None,
    })
}

impl Default for HotspotConfig {
    fn default() -> Self {
        Self {
            ssid: "Nimbus-Hotspot".to_string(),
            password: String::new(),
            band: Band::Auto,
            channel: None,
            // Deliberately not a country: changing the regulatory domain is a
            // machine-wide side effect and must be asked for explicitly.
            country_code: None,
            security: Security::Wpa2Wpa3Transition,
            hidden: false,
            max_clients: None,
            client_isolation: false,
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

        // The SSID and passphrase are written verbatim into hostapd and
        // dnsmasq configuration files, where a newline would start a new
        // directive. Reject anything that could break out of its line.
        if self.ssid.chars().any(|c| c.is_control()) {
            return Err(crate::NimbusError::InvalidValue(
                "SSID must not contain control characters".into(),
            ));
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
            if self.password.chars().any(|c| c.is_control()) {
                return Err(crate::NimbusError::InvalidValue(
                    "Password must not contain control characters".into(),
                ));
            }
        }

        if let Some(max) = self.max_clients {
            if max == 0 {
                return Err(crate::NimbusError::InvalidValue(
                    "Max clients must be at least 1 (leave unset for no limit)".into(),
                ));
            }
            if max > MAX_CLIENTS_MAX {
                return Err(crate::NimbusError::InvalidValue(format!(
                    "Max clients must be at most {}",
                    MAX_CLIENTS_MAX
                )));
            }
        }

        if let Some(code) = &self.country_code {
            let valid =
                code == "00" || (code.len() == 2 && code.chars().all(|c| c.is_ascii_alphabetic()));
            if !valid {
                return Err(crate::NimbusError::InvalidValue(format!(
                    "Country code '{}' must be two letters (ISO 3166-1 alpha-2)",
                    code
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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
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
    /// Whether these capabilities were actually read from the radio.
    ///
    /// `false` means `iw` was unavailable or refused to answer, so every
    /// capability flag below is a best-effort guess rather than a fact.
    /// Callers must not reject a configuration based on guessed values.
    pub detected: bool,
    /// The radio advertises an interface combination holding a managed (STA)
    /// and an AP interface at the same time.
    pub supports_simultaneous_sta_ap: bool,
    /// That combination is limited to `#channels <= 1`: the AP can only stay up
    /// alongside the STA connection if both sit on the same channel.
    pub sta_ap_same_channel_only: bool,
    pub supported_bands: Vec<Band>,
    pub supported_channels_2ghz: Vec<u32>,
    pub supported_channels_5ghz: Vec<u32>,
    pub max_sta: u32,
}

/// A client ("station" / managed mode) Wi-Fi connection currently held by an
/// adapter — i.e. the network this machine is joined to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StationConnection {
    pub interface: String,
    pub ssid: Option<String>,
    pub frequency: u32,
}

impl StationConnection {
    pub fn channel(&self) -> u32 {
        freq_to_channel(self.frequency)
    }
}

/// What starting the hotspot would do to an existing station connection living
/// on the same adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaImpact {
    /// The AP interface carries no station connection, so there is nothing to
    /// lose.
    None,
    /// Bringing the AP up tears down the station connection, and with it the
    /// upstream this machine is using.
    ///
    /// This is the outcome whenever the chosen adapter is already joined to a
    /// network, even on radios whose interface-combination table allows an AP
    /// and a station at once: NetworkManager activates the AP profile on the
    /// same network device, and a device can only hold one active connection.
    /// Exploiting the hardware's concurrency would require a second virtual
    /// interface, which Nimbus does not create.
    DisconnectsUplink { ssid: Option<String> },
}

impl StaImpact {
    /// True when acting on this costs the machine its station connection.
    pub fn is_disconnecting(&self) -> bool {
        matches!(self, StaImpact::DisconnectsUplink { .. })
    }
}

/// How the access point will actually be brought up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotspotBackend {
    /// A NetworkManager AP profile on the adapter itself.
    ///
    /// Needs no extra tools or privileges, but NetworkManager reuses the same
    /// network device, so any station connection on that adapter is dropped.
    NetworkManager,
    /// hostapd on a second, virtual AP interface carved out of the same radio.
    ///
    /// Keeps the machine's own Wi-Fi connection and shares it with clients.
    /// Needs root, hostapd and dnsmasq, and pins the AP to the channel the
    /// station connection is using.
    SharedVirtualAp,
}

impl fmt::Display for HotspotBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HotspotBackend::NetworkManager => write!(f, "NetworkManager"),
            HotspotBackend::SharedVirtualAp => write!(f, "shared (hostapd)"),
        }
    }
}

/// A machine-wide regulatory domain switch the plan would carry out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegDomainChange {
    pub from: Option<String>,
    pub to: String,
}

/// The resolved, side-effect-free outcome of working out how a hotspot would be
/// brought up. Produced before anything touches NetworkManager.
#[derive(Debug, Clone)]
pub struct HotspotPlan {
    pub ap_interface: String,
    pub upstream_interface: Option<String>,
    /// Which mechanism will create the access point.
    pub backend: HotspotBackend,
    /// The channel the AP will use, when the plan has to pin one.
    pub channel: Option<u32>,
    /// `config` with any adjustments the plan had to make (channel pinning).
    pub effective_config: HotspotConfig,
    pub impact: StaImpact,
    /// Set when the hotspot needs a different regulatory domain than the one
    /// the machine is currently using.
    pub regdomain_change: Option<RegDomainChange>,
    /// Everything that stopped the shared backend from being used, phrased for
    /// the user (missing root, hostapd, dnsmasq, …). Empty when sharing was
    /// possible — or not applicable because no connection was at stake.
    pub share_blockers: Vec<String>,
    pub warnings: Vec<String>,
}

impl HotspotPlan {
    /// True when carrying out this plan costs the user their current uplink.
    pub fn needs_confirmation(&self) -> bool {
        self.impact.is_disconnecting()
    }
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
            country_code: None,
            security: Security::Wpa2,
            hidden: false,
            max_clients: Some(10),
            client_isolation: false,
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
    fn test_max_clients_zero_fails() {
        // Zero would mean "no devices allowed"; unlimited is expressed as None.
        let mut config = valid_config();
        config.max_clients = Some(0);
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_max_clients_unlimited_passes() {
        let mut config = valid_config();
        config.max_clients = None;
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_default_config_leaves_the_regulatory_domain_alone() {
        // Changing the regulatory domain affects every adapter on the machine,
        // so it must never happen unless it was asked for.
        assert_eq!(HotspotConfig::default().country_code, None);
    }

    #[test]
    fn test_valid_country_codes_pass() {
        for code in ["FR", "US", "DE", "00"] {
            let mut config = valid_config();
            config.country_code = Some(code.to_string());
            assert!(config.validate().is_ok(), "{} should be accepted", code);
        }
    }

    #[test]
    fn test_invalid_country_codes_fail() {
        for code in ["USA", "F", "", "1A"] {
            let mut config = valid_config();
            config.country_code = Some(code.to_string());
            assert!(config.validate().is_err(), "{} should be rejected", code);
        }
    }

    #[test]
    fn test_country_code_deserializes_from_a_bare_string() {
        // Hotspots saved by earlier versions stored a plain string.
        let json = r#"{"ssid":"Old","password":"password123","band":"Auto",
            "channel":null,"country_code":"fr","security":"Wpa2","hidden":false,
            "max_clients":10,"client_isolation":false,
            "auto_start":false}"#;
        let config: HotspotConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.country_code.as_deref(), Some("FR"));
    }

    #[test]
    fn test_country_code_deserializes_when_absent() {
        let json = r#"{"ssid":"New","password":"password123","band":"Auto",
            "channel":null,"security":"Wpa2","hidden":false,
            "max_clients":null,"client_isolation":false,
            "auto_start":false}"#;
        let config: HotspotConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.country_code, None);
    }

    #[test]
    fn test_country_code_empty_string_means_unset() {
        let json = r#"{"ssid":"New","password":"password123","band":"Auto",
            "channel":null,"country_code":"","security":"Wpa2","hidden":false,
            "max_clients":null,"client_isolation":false,
            "auto_start":false}"#;
        let config: HotspotConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.country_code, None);
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
        assert_eq!(
            HotspotState::Active("test".into()),
            HotspotState::Active("test".into())
        );
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
