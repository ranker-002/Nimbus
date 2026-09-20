use std::collections::HashMap;

use async_trait::async_trait;
use mac_address::MacAddress;
use tokio::process::Command;

use nimbus_core::error::{NimbusError, Result};
use nimbus_core::types::{
    channel_to_freq, freq_to_channel, AdapterCapabilities, Band, HotspotConfig, HotspotInfo,
    InterfaceState, InterfaceType, NetworkInterface, Security, StationConnection, StationInfo,
};

use crate::capabilities::parse_iw_phy_info;
use crate::interface;
use crate::station;
use crate::traits::NetworkManagerApi;

pub fn parse_mac(s: &str) -> MacAddress {
    let cleaned: String = s
        .chars()
        .filter(|c| c.is_ascii_hexdigit() || *c == ':')
        .collect();
    let bytes: Vec<u8> = cleaned
        .split(':')
        .filter_map(|h| u8::from_str_radix(h, 16).ok())
        .collect();
    if bytes.len() == 6 {
        let mut arr = [0u8; 6];
        arr.copy_from_slice(&bytes);
        MacAddress::new(arr)
    } else {
        MacAddress::new([0; 6])
    }
}

/// Prefix stamped on the `connection.id` of every NetworkManager profile
/// Nimbus creates.
///
/// These profiles are runtime artefacts owned entirely by Nimbus — saved
/// hotspot configurations live in `~/.config/nimbus-hotspot/`, not in
/// NetworkManager — so anything carrying this prefix can be removed once it is
/// no longer active.
pub const PROFILE_PREFIX: &str = "Nimbus-";

/// Whether a NetworkManager profile id belongs to Nimbus.
pub fn is_nimbus_profile(id: &str) -> bool {
    id.starts_with(PROFILE_PREFIX)
}

/// The id and uuid of every active connection Nimbus created.
///
/// NetworkManager appends a counter when an id is already taken
/// (`Nimbus-Foo 1`), so matching is done on the prefix rather than the exact
/// name.
fn active_nimbus_profiles(active: &[nmrs::ActiveConnection]) -> Vec<(String, String)> {
    active
        .iter()
        .filter_map(|ac| match ac {
            nmrs::ActiveConnection::Wifi(c) => Some((&c.id, &c.uuid)),
            nmrs::ActiveConnection::Wired(c) => Some((&c.id, &c.uuid)),
            _ => None,
        })
        .filter(|(id, _)| is_nimbus_profile(id))
        .map(|(id, uuid)| (id.clone(), uuid.clone()))
        .collect()
}

/// The `802-11-wireless-security.key-mgmt` value NetworkManager expects, or
/// `None` when the connection must carry no security setting at all.
///
/// Two traps live here:
///
/// - NetworkManager takes exactly **one** value — a space-separated list like
///   `"wpa-psk sae"` is rejected with `InvalidProperty`. A genuine WPA2/WPA3
///   transition AP needs hostapd's `wpa_key_mgmt=WPA-PSK SAE`, which
///   NetworkManager cannot express, so the transition setting degrades to WPA2.
/// - An open network is *not* `key-mgmt="none"`. That value means static WEP,
///   and wpa_supplicant rejects it with "does not support WEP encryption". An
///   open network is expressed by omitting `802-11-wireless-security` entirely.
///
/// [`nimbus_backend::planner`] rewrites the config for the first case, so what
/// the user is shown matches what is created.
pub fn key_mgmt_for(security: &Security) -> Option<&'static str> {
    match security {
        Security::Open => None,
        Security::Wpa3 => Some("sae"),
        // WPA2, and the transition setting that has to degrade to it.
        Security::Wpa2 | Security::Wpa2Wpa3Transition => Some("wpa-psk"),
    }
}

pub fn validate_mac(s: &str) -> bool {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 6 {
        return false;
    }
    parts
        .iter()
        .all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_hexdigit()))
}

pub struct NmManager {
    client: Option<nmrs::NetworkManager>,
}

impl NmManager {
    pub async fn new() -> Self {
        let client = nmrs::NetworkManager::new().await.ok();
        Self { client }
    }

    fn require_client(&self) -> Result<&nmrs::NetworkManager> {
        self.client
            .as_ref()
            .ok_or_else(|| NimbusError::NetworkManagerUnavailable("nmrs init failed".into()))
    }
}

#[async_trait]
impl NetworkManagerApi for NmManager {
    async fn get_wifi_devices(&self) -> Result<Vec<NetworkInterface>> {
        let nm = self.require_client()?;
        let devices = nm.list_wifi_devices().await.map_err(|e| {
            NimbusError::NetworkManagerUnavailable(format!("Failed to list devices: {}", e))
        })?;

        let mut result = Vec::new();
        for dev in &devices {
            let mac = parse_mac(&dev.hw_address);
            let state = match dev.state {
                nmrs::DeviceState::Activated => InterfaceState::Up,
                nmrs::DeviceState::Disconnected => InterfaceState::Disconnected,
                nmrs::DeviceState::Unavailable => InterfaceState::Unavailable,
                _ => InterfaceState::Down,
            };

            result.push(NetworkInterface {
                name: dev.interface.clone(),
                interface_type: InterfaceType::Wifi,
                mac,
                state,
                driver: dev.driver.clone().unwrap_or_default(),
            });
        }
        Ok(result)
    }

    async fn get_all_interfaces(&self) -> Result<Vec<NetworkInterface>> {
        let nm = self.require_client()?;
        let devices = nm.list_devices().await.map_err(|e| {
            NimbusError::NetworkManagerUnavailable(format!("Failed to list devices: {}", e))
        })?;

        let mut result = Vec::new();
        for dev in &devices {
            let iface_type = if dev.is_wireless() {
                InterfaceType::Wifi
            } else if dev.is_wired() {
                InterfaceType::Ethernet
            } else {
                InterfaceType::Unknown
            };

            let mac = parse_mac(&dev.identity.current_mac);

            let state = match dev.state {
                nmrs::DeviceState::Activated => InterfaceState::Up,
                nmrs::DeviceState::Disconnected => InterfaceState::Disconnected,
                nmrs::DeviceState::Unavailable => InterfaceState::Unavailable,
                _ => InterfaceState::Down,
            };

            result.push(NetworkInterface {
                name: dev.interface.clone(),
                interface_type: iface_type,
                mac,
                state,
                driver: dev.driver.clone().unwrap_or_default(),
            });
        }
        Ok(result)
    }

    async fn get_adapter_capabilities(&self, interface: &str) -> Result<AdapterCapabilities> {
        let nm = self.require_client()?;
        let _wifi_dev = nm.wifi_device_by_interface(interface).await.map_err(|e| {
            NimbusError::InterfaceNotFound(format!("Device '{}' not found: {}", interface, e))
        })?;

        let (iw_caps, detected) = match parse_iw_phy_info(interface).await {
            Ok(caps) => (caps, true),
            Err(e) => {
                // `iw` is not installed or refused to answer. Guessing is
                // better than blocking: NetworkManager can still create the
                // AP, and the planner knows not to reject the request based on
                // capabilities it does not actually have.
                log::warn!(
                    "Could not read the capabilities of {} ({}); starting from \
                     conservative defaults",
                    interface,
                    e
                );
                (
                    crate::capabilities::IwPhyInfo {
                        supports_ap: true,
                        supports_wpa3: true,
                        ..Default::default()
                    },
                    false,
                )
            }
        };
        let phy_name = self.get_phy_name(interface).await.unwrap_or_default();
        let driver = self.get_driver(interface).await.unwrap_or_default();

        let mut supported_bands = Vec::new();
        if !iw_caps.channels_2ghz.is_empty() {
            supported_bands.push(Band::Band2_4Ghz);
        }
        if !iw_caps.channels_5ghz.is_empty() {
            supported_bands.push(Band::Band5Ghz);
        }

        Ok(AdapterCapabilities {
            interface: interface.to_string(),
            phy_name,
            driver,
            supports_ap: iw_caps.supports_ap,
            supports_wpa3: iw_caps.supports_wpa3,
            supports_wifi_6: iw_caps.supports_wifi_6,
            supports_wifi_6e: iw_caps.supports_wifi_6e,
            supports_wifi_7: iw_caps.supports_wifi_7,
            detected,
            supports_simultaneous_sta_ap: iw_caps.can_do_sta_and_ap,
            sta_ap_same_channel_only: iw_caps.sta_ap_same_channel_only,
            supported_bands,
            supported_channels_2ghz: iw_caps.channels_2ghz,
            supported_channels_5ghz: iw_caps.channels_5ghz,
            max_sta: iw_caps.max_sta,
        })
    }

    async fn create_hotspot(&self, config: &HotspotConfig, interface: &str) -> Result<HotspotInfo> {
        config.validate()?;
        let nm = self.require_client()?;

        let mut settings: HashMap<&str, HashMap<&str, zbus::zvariant::Value<'_>>> = HashMap::new();

        // Clear out profiles left behind by an earlier run before adding
        // another, so repeated starts do not pile up "Nimbus-Foo 1",
        // "Nimbus-Foo 2", ... in NetworkManager.
        self.purge_stale_profiles().await;

        let mut conn = HashMap::new();
        conn.insert("type", zbus::zvariant::Value::Str("802-11-wireless".into()));
        conn.insert(
            "id",
            zbus::zvariant::Value::Str(format!("{}{}", PROFILE_PREFIX, config.ssid).into()),
        );
        // Never let NetworkManager bring the hotspot up on its own. On a
        // single-radio machine an autoconnecting AP profile would seize the
        // adapter after a reboot or a rfkill toggle and drop the user's Wi-Fi
        // with no interaction at all.
        conn.insert("autoconnect", zbus::zvariant::Value::Bool(false));
        settings.insert("connection", conn);

        let mut wifi = HashMap::new();
        let ssid_bytes: Vec<u8> = config.ssid.as_bytes().to_vec();
        wifi.insert(
            "ssid",
            zbus::zvariant::Value::Array(zbus::zvariant::Array::from(ssid_bytes)),
        );
        wifi.insert("mode", zbus::zvariant::Value::Str("ap".into()));

        // NetworkManager rejects a channel without a band, so when a specific
        // channel is pinned and the band is left on Auto, derive it.
        let effective_band = match (&config.band, config.channel) {
            (Band::Auto, Some(ch)) => Band::of_channel(ch),
            (band, _) => band.clone(),
        };

        match effective_band {
            Band::Band2_4Ghz => {
                wifi.insert("band", zbus::zvariant::Value::Str("bg".into()));
            }
            Band::Band5Ghz => {
                wifi.insert("band", zbus::zvariant::Value::Str("a".into()));
            }
            Band::Auto => {}
        }

        if let Some(ch) = config.channel {
            wifi.insert("channel", zbus::zvariant::Value::U32(ch));
        }
        if config.hidden {
            wifi.insert("hidden", zbus::zvariant::Value::Bool(true));
        }
        if config.client_isolation {
            wifi.insert("ap-isolation", zbus::zvariant::Value::I32(1));
        }

        settings.insert("802-11-wireless", wifi);

        // Omitted entirely for an open network — see `key_mgmt_for`.
        if let Some(key_mgmt) = key_mgmt_for(&config.security) {
            let mut wsec = HashMap::new();
            wsec.insert("key-mgmt", zbus::zvariant::Value::Str(key_mgmt.into()));
            wsec.insert(
                "psk",
                zbus::zvariant::Value::Str(config.password.as_str().into()),
            );
            settings.insert("802-11-wireless-security", wsec);
        }

        let mut ip4 = HashMap::new();
        ip4.insert("method", zbus::zvariant::Value::Str("shared".into()));
        settings.insert("ipv4", ip4);

        let mut ip6 = HashMap::new();
        ip6.insert("method", zbus::zvariant::Value::Str("ignore".into()));
        settings.insert("ipv6", ip6);

        let _result = nm
            .add_and_activate_connection(settings, Some(interface), Some("/"))
            .await
            .map_err(|e| {
                NimbusError::HotspotCreationFailed(format!("Failed to create connection: {}", e))
            })?;

        // Wait for connection to activate by polling device state
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if std::time::Instant::now() >= deadline {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            if let Ok(devices) = nm.list_wifi_devices().await {
                if let Some(dev) = devices.iter().find(|d| d.interface == interface) {
                    if dev.state == nmrs::DeviceState::Activated {
                        break;
                    }
                }
            }
        }

        // Report what the device actually settled on; fall back to the
        // configured channel when NM has not published a frequency yet.
        let live_frequency = nm
            .list_wifi_devices()
            .await
            .ok()
            .and_then(|devices| {
                devices
                    .iter()
                    .find(|d| d.interface == interface)
                    .and_then(|d| d.active_frequency_mhz)
            })
            .filter(|f| *f > 0);

        let (frequency, channel) = match live_frequency {
            Some(freq) => (freq, freq_to_channel(freq)),
            None => match (&config.band, config.channel) {
                (_, Some(ch)) => (channel_to_freq(ch), ch),
                (Band::Band5Ghz, None) => (5180, 36),
                (Band::Band2_4Ghz | Band::Auto, None) => (2437, 6),
            },
        };

        Ok(HotspotInfo {
            interface: interface.to_string(),
            ssid: config.ssid.clone(),
            ip: std::net::Ipv4Addr::new(10, 42, 0, 1),
            frequency,
            channel,
            started_at: std::time::Instant::now(),
        })
    }

    async fn stop_hotspot(&self) -> Result<()> {
        let nm = self.require_client()?;
        let active = nm
            .list_active_connections()
            .await
            .map_err(|_e| NimbusError::HotspotNotActive)?;

        let running = active_nimbus_profiles(&active);
        if running.is_empty() {
            // Still sweep up: a profile can survive a crash between start and
            // stop without ever being active again.
            self.purge_stale_profiles().await;
            return Err(NimbusError::HotspotNotActive);
        }

        for (id, uuid) in &running {
            // Bring it down first so clients are dropped cleanly. Deleting the
            // profile also tears the connection down, so a failure here is not
            // fatal — the delete below is what actually guarantees it.
            if let Err(e) = self.deactivate_connection_by_uuid(uuid).await {
                log::warn!("Could not deactivate '{}': {}", id, e);
            }
            if let Err(e) = nm.delete_saved_connection(uuid).await {
                log::warn!("Could not remove the profile for '{}': {}", id, e);
            }
        }

        // Catch anything that was already inactive.
        self.purge_stale_profiles().await;
        Ok(())
    }

    async fn get_active_hotspot(&self) -> Result<Option<HotspotInfo>> {
        let nm = self.require_client()?;
        let active = nm
            .list_active_connections()
            .await
            .map_err(|e| NimbusError::NetworkManagerUnavailable(format!("{}", e)))?;

        for ac in &active {
            if let nmrs::ActiveConnection::Wifi(wifi) = ac {
                if is_nimbus_profile(&wifi.id)
                    && wifi.state == nmrs::ActiveConnectionState::Activated
                {
                    let iface = wifi.interface.clone().unwrap_or_default();
                    let ssid = wifi.ssid.clone();

                    // Try to get actual frequency from the device
                    let (frequency, channel) = if let Ok(devices) = nm.list_wifi_devices().await {
                        if let Some(dev) = devices.iter().find(|d| d.interface == iface) {
                            let freq = dev.active_frequency_mhz.unwrap_or(2412);
                            let ch = freq_to_channel(freq);
                            (freq, ch)
                        } else {
                            (2412, 1)
                        }
                    } else {
                        (2412, 1)
                    };

                    return Ok(Some(HotspotInfo {
                        interface: iface,
                        ssid,
                        ip: std::net::Ipv4Addr::new(10, 42, 0, 1),
                        frequency,
                        channel,
                        started_at: std::time::Instant::now(),
                    }));
                }
            }
        }
        Ok(None)
    }

    async fn get_connected_stations(&self, interface: &str) -> Result<Vec<StationInfo>> {
        station::get_stations(interface).await
    }

    async fn disconnect_station(&self, interface: &str, mac: &MacAddress) -> Result<()> {
        station::disconnect_station(interface, mac).await
    }

    async fn get_upstream_interface(&self) -> Result<Option<String>> {
        interface::get_upstream_interface().await
    }

    async fn get_station_connection(&self, interface: &str) -> Result<Option<StationConnection>> {
        let nm = self.require_client()?;

        // A device hosting one of our hotspots is acting as an AP, not as a
        // station. This has to be decided from the profile id: in AP mode
        // NetworkManager reports the hotspot's own SSID as the device's active
        // SSID, which is whatever the user typed and carries no marker.
        if let Ok(active) = nm.list_active_connections().await {
            let hosts_our_ap = active.iter().any(|ac| {
                matches!(ac, nmrs::ActiveConnection::Wifi(c)
                    if is_nimbus_profile(&c.id) && c.interface.as_deref() == Some(interface))
            });
            if hosts_our_ap {
                return Ok(None);
            }
        }

        let devices = nm.list_wifi_devices().await.map_err(|e| {
            NimbusError::NetworkManagerUnavailable(format!("Failed to list devices: {}", e))
        })?;

        let Some(dev) = devices.iter().find(|d| d.interface == interface) else {
            return Ok(None);
        };

        // Only an activated device is actually joined to a network.
        if dev.state != nmrs::DeviceState::Activated {
            return Ok(None);
        }

        Ok(dev.active_frequency_mhz.map(|frequency| StationConnection {
            interface: interface.to_string(),
            ssid: dev.active_ssid.clone(),
            frequency,
        }))
    }

    async fn is_nm_available(&self) -> bool {
        self.client.is_some()
    }
}

impl NmManager {
    /// Deletes Nimbus profiles that are not currently in use.
    ///
    /// Active ones are left alone, so this can run while a hotspot from another
    /// Nimbus instance is up. Returns how many were removed; failures are
    /// logged rather than raised, since cleanup must never stop a hotspot from
    /// starting or stopping.
    async fn purge_stale_profiles(&self) -> usize {
        let Ok(nm) = self.require_client() else {
            return 0;
        };

        let in_use: Vec<String> = match nm.list_active_connections().await {
            Ok(active) => active_nimbus_profiles(&active)
                .into_iter()
                .map(|(_, uuid)| uuid)
                .collect(),
            Err(e) => {
                log::warn!("Could not list active connections: {}", e);
                return 0;
            }
        };

        let saved = match nm.list_saved_connections_brief().await {
            Ok(saved) => saved,
            Err(e) => {
                log::warn!("Could not list saved connections: {}", e);
                return 0;
            }
        };

        let mut removed = 0;
        for profile in saved
            .iter()
            .filter(|p| is_nimbus_profile(&p.id) && !in_use.contains(&p.uuid))
        {
            match nm.delete_saved_connection(&profile.uuid).await {
                Ok(()) => {
                    log::debug!("Removed leftover profile '{}'", profile.id);
                    removed += 1;
                }
                Err(e) => log::warn!("Could not remove profile '{}': {}", profile.id, e),
            }
        }
        removed
    }

    async fn deactivate_connection_by_uuid(&self, uuid: &str) -> Result<()> {
        let output = Command::new("nmcli")
            .args(["connection", "down", "uuid", uuid])
            .output()
            .await
            .map_err(|e| NimbusError::NftablesError(format!("Failed to run nmcli: {}", e)))?;

        if output.status.success() {
            Ok(())
        } else {
            Err(NimbusError::HotspotCreationFailed(format!(
                "Failed to deactivate connection: {}",
                String::from_utf8_lossy(&output.stderr)
            )))
        }
    }

    async fn get_phy_name(&self, interface: &str) -> Result<String> {
        let output = Command::new("iw")
            .args(["dev", interface, "info"])
            .output()
            .await?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if let Some(wphy) = line
                .strip_prefix("\twiphy ")
                .or_else(|| line.strip_prefix("wiphy "))
            {
                return Ok(format!("phy{}", wphy.trim()));
            }
        }
        Err(NimbusError::IwError(format!(
            "Could not determine PHY for {}",
            interface
        )))
    }

    async fn get_driver(&self, interface: &str) -> Result<String> {
        let driver_path = format!("/sys/class/net/{}/device/driver/module", interface);
        let output = tokio::fs::read_link(&driver_path).await;
        match output {
            Ok(path) => {
                let driver = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
                    .to_string();
                Ok(driver)
            }
            Err(_) => Ok("unknown".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_profiles_nimbus_created() {
        assert!(is_nimbus_profile("Nimbus-MonHotspot"));
        // NetworkManager appends a counter when the id is already taken.
        assert!(is_nimbus_profile("Nimbus-MonHotspot 1"));
        // An empty SSID still carries the prefix.
        assert!(is_nimbus_profile("Nimbus-"));
    }

    #[test]
    fn leaves_profiles_it_does_not_own_alone() {
        // The user's own networks must never be swept up by the cleanup, even
        // when the name looks similar.
        assert!(!is_nimbus_profile("CANALBOX-E7E2-2G-5G"));
        assert!(!is_nimbus_profile("Nimbus"));
        assert!(!is_nimbus_profile("My Nimbus-Hotspot"));
        assert!(!is_nimbus_profile("nimbus-lowercase"));
        assert!(!is_nimbus_profile(""));
    }

    #[test]
    fn parses_a_well_formed_mac() {
        assert_eq!(
            parse_mac("14:13:33:37:4b:89"),
            MacAddress::new([0x14, 0x13, 0x33, 0x37, 0x4b, 0x89])
        );
    }

    #[test]
    fn falls_back_to_a_zero_mac_when_unparseable() {
        assert_eq!(parse_mac("not-a-mac"), MacAddress::new([0; 6]));
        assert_eq!(parse_mac(""), MacAddress::new([0; 6]));
    }

    #[test]
    fn validates_mac_formatting() {
        assert!(validate_mac("14:13:33:37:4B:89"));
        assert!(!validate_mac("14:13:33:37:4B"));
        assert!(!validate_mac("14-13-33-37-4B-89"));
        assert!(!validate_mac("14:13:33:37:4B:8G"));
    }
}

#[cfg(test)]
mod key_mgmt_tests {
    use super::*;

    /// Every value must be one NetworkManager accepts. A space-separated list
    /// is rejected with `InvalidProperty`, which is what used to make every
    /// hotspot start fail out of the box.
    #[test]
    fn key_mgmt_is_always_a_single_value() {
        for security in [
            Security::Open,
            Security::Wpa2,
            Security::Wpa3,
            Security::Wpa2Wpa3Transition,
        ] {
            let Some(value) = key_mgmt_for(&security) else {
                continue;
            };
            assert!(
                !value.contains(' '),
                "{:?} produced the multi-value key-mgmt {:?}",
                security,
                value
            );
            assert!(["wpa-psk", "sae"].contains(&value));
        }
    }

    /// `key-mgmt="none"` is static WEP in NetworkManager's vocabulary, and
    /// wpa_supplicant refuses it outright. An open network must carry no
    /// security setting at all.
    #[test]
    fn an_open_network_has_no_key_mgmt() {
        assert_eq!(key_mgmt_for(&Security::Open), None);
    }

    #[test]
    fn key_mgmt_maps_each_secured_mode() {
        assert_eq!(key_mgmt_for(&Security::Wpa2), Some("wpa-psk"));
        assert_eq!(key_mgmt_for(&Security::Wpa3), Some("sae"));
        // No mixed AP support in NetworkManager: falls back to WPA2.
        assert_eq!(key_mgmt_for(&Security::Wpa2Wpa3Transition), Some("wpa-psk"));
    }

    #[test]
    fn no_security_mode_ever_asks_for_wep() {
        for security in [
            Security::Open,
            Security::Wpa2,
            Security::Wpa3,
            Security::Wpa2Wpa3Transition,
        ] {
            assert_ne!(key_mgmt_for(&security), Some("none"));
        }
    }
}
