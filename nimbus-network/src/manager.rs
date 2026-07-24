use std::collections::HashMap;

use async_trait::async_trait;
use mac_address::MacAddress;
use tokio::process::Command;

use nimbus_core::error::{NimbusError, Result};
use nimbus_core::types::{
    AdapterCapabilities, Band, BandwidthSample, HotspotConfig, HotspotInfo, InterfaceState,
    InterfaceType, NetworkInterface, StationInfo,
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

pub fn validate_mac(s: &str) -> bool {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 6 {
        return false;
    }
    parts.iter().all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_hexdigit()))
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

        let iw_caps = parse_iw_phy_info(interface).await.unwrap_or_default();
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
            supports_simultaneous_sta_ap: iw_caps.can_do_sta_and_ap,
            supported_bands,
            supported_channels_2ghz: iw_caps.channels_2ghz,
            supported_channels_5ghz: iw_caps.channels_5ghz,
            max_sta: iw_caps.max_sta,
        })
    }

    async fn create_hotspot(
        &self,
        config: &HotspotConfig,
        interface: &str,
    ) -> Result<HotspotInfo> {
        config.validate()?;
        let nm = self.require_client()?;

        let mut settings: HashMap<&str, HashMap<&str, zbus::zvariant::Value<'_>>> = HashMap::new();

        let mut conn = HashMap::new();
        conn.insert(
            "type",
            zbus::zvariant::Value::Str("802-11-wireless".into()),
        );
        conn.insert(
            "id",
            zbus::zvariant::Value::Str(format!("Nimbus-{}", config.ssid).into()),
        );
        settings.insert("connection", conn);

        let mut wifi = HashMap::new();
        let ssid_bytes: Vec<u8> = config.ssid.as_bytes().to_vec();
        wifi.insert(
            "ssid",
            zbus::zvariant::Value::Array(zbus::zvariant::Array::from(ssid_bytes)),
        );
        wifi.insert("mode", zbus::zvariant::Value::Str("ap".into()));

        match config.band {
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

        let mut wsec = HashMap::new();
        match config.security {
            nimbus_core::types::Security::Open => {
                wsec.insert(
                    "key-mgmt",
                    zbus::zvariant::Value::Str("none".into()),
                );
            }
            nimbus_core::types::Security::Wpa2 => {
                wsec.insert(
                    "key-mgmt",
                    zbus::zvariant::Value::Str("wpa-psk".into()),
                );
                wsec.insert(
                    "psk",
                    zbus::zvariant::Value::Str(config.password.as_str().into()),
                );
            }
            nimbus_core::types::Security::Wpa3 => {
                wsec.insert("key-mgmt", zbus::zvariant::Value::Str("sae".into()));
                wsec.insert(
                    "psk",
                    zbus::zvariant::Value::Str(config.password.as_str().into()),
                );
            }
            nimbus_core::types::Security::Wpa2Wpa3Transition => {
                wsec.insert(
                    "key-mgmt",
                    zbus::zvariant::Value::Str("wpa-psk sae".into()),
                );
                wsec.insert(
                    "psk",
                    zbus::zvariant::Value::Str(config.password.as_str().into()),
                );
            }
        }
        settings.insert("802-11-wireless-security", wsec);

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

        // Determine frequency and channel from config
        let (frequency, channel) = match (&config.band, config.channel) {
            (Band::Band5Ghz, Some(ch)) => (5000 + ch * 5, ch),
            (Band::Band5Ghz, None) => (5180, 36),
            (Band::Band2_4Ghz, Some(ch)) => (2407 + ch * 5, ch),
            (Band::Band2_4Ghz, None) => (2437, 6),
            (Band::Auto, Some(ch)) => {
                if ch >= 36 {
                    (5000 + ch * 5, ch)
                } else {
                    (2407 + ch * 5, ch)
                }
            }
            (Band::Auto, None) => (2437, 6),
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
        let active = nm.list_active_connections().await.map_err(|_e| {
            NimbusError::HotspotNotActive
        })?;

        for ac in &active {
            match ac {
                nmrs::ActiveConnection::Wifi(wifi) => {
                    if wifi.id.starts_with("Nimbus-") {
                        self.deactivate_connection_by_uuid(&wifi.uuid).await?;
                        return Ok(());
                    }
                }
                nmrs::ActiveConnection::Wired(wired)
                    if wired.id.starts_with("Nimbus-") => {
                    self.deactivate_connection_by_uuid(&wired.uuid).await?;
                    return Ok(());
                }
                _ => {}
            }
        }
        Err(NimbusError::HotspotNotActive)
    }

    async fn get_active_hotspot(&self) -> Result<Option<HotspotInfo>> {
        let nm = self.require_client()?;
        let active = nm.list_active_connections().await.map_err(|e| {
            NimbusError::NetworkManagerUnavailable(format!("{}", e))
        })?;

        for ac in &active {
            if let nmrs::ActiveConnection::Wifi(wifi) = ac {
                if wifi.id.starts_with("Nimbus-")
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

    async fn get_upstream_interface(&self) -> Result<Option<String>> {
        let interfaces = interface::detect_interfaces().await?;
        for iface in &interfaces {
            if iface.interface_type == InterfaceType::Ethernet
                && iface.state == InterfaceState::Up
            {
                return Ok(Some(iface.name.clone()));
            }
        }
        for iface in &interfaces {
            if iface.interface_type == InterfaceType::Wifi && iface.state == InterfaceState::Up {
                return Ok(Some(iface.name.clone()));
            }
        }
        Ok(None)
    }

    async fn get_bandwidth_sample(&self, interface: &str) -> Result<BandwidthSample> {
        let rx_path = format!("/sys/class/net/{}/statistics/rx_bytes", interface);
        let tx_path = format!("/sys/class/net/{}/statistics/tx_bytes", interface);

        let rx = tokio::fs::read_to_string(&rx_path)
            .await?
            .trim()
            .parse::<u64>()
            .unwrap_or(0);
        let tx = tokio::fs::read_to_string(&tx_path)
            .await?
            .trim()
            .parse::<u64>()
            .unwrap_or(0);

        Ok(BandwidthSample {
            rx_rate: 0,
            tx_rate: 0,
            total_rx: rx,
            total_tx: tx,
        })
    }

    async fn is_nm_available(&self) -> bool {
        self.client.is_some()
    }
}

fn freq_to_channel(freq: u32) -> u32 {
    match freq {
        2412 => 1,
        2417 => 2,
        2422 => 3,
        2427 => 4,
        2432 => 5,
        2437 => 6,
        2442 => 7,
        2447 => 8,
        2452 => 9,
        2457 => 10,
        2462 => 11,
        2467 => 12,
        2472 => 13,
        f if (5170..=5825).contains(&f) => (f - 5000) / 5,
        f if (5955..=7115).contains(&f) => (f - 5950) / 5,
        _ => 0,
    }
}

impl NmManager {
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
