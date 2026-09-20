//! A stand-in for NetworkManager, so the planning and enforcement rules can be
//! exercised without touching the machine's networking.

use std::collections::HashMap;
use std::sync::Mutex;

use mac_address::MacAddress;
use nimbus_core::error::{NimbusError, Result};
use nimbus_core::types::{
    AdapterCapabilities, Band, HotspotConfig, HotspotInfo, InterfaceState, InterfaceType,
    NetworkInterface, StationConnection, StationInfo,
};
use nimbus_network::traits::NetworkManagerApi;

pub struct FakeNm {
    pub devices: Vec<NetworkInterface>,
    pub caps: HashMap<String, AdapterCapabilities>,
    pub stations: HashMap<String, StationConnection>,
    pub upstream: Option<String>,
    pub connected: Mutex<Vec<StationInfo>>,
    /// Every MAC passed to `disconnect_station`, in order.
    pub disconnected: Mutex<Vec<MacAddress>>,
    /// When set, `disconnect_station` fails — as it does without CAP_NET_ADMIN.
    pub disconnect_fails: bool,
}

impl FakeNm {
    pub fn new() -> Self {
        Self {
            devices: Vec::new(),
            caps: HashMap::new(),
            stations: HashMap::new(),
            upstream: None,
            connected: Mutex::new(Vec::new()),
            disconnected: Mutex::new(Vec::new()),
            disconnect_fails: false,
        }
    }

    pub fn with_adapter(mut self, name: &str, caps: AdapterCapabilities) -> Self {
        self.devices.push(NetworkInterface {
            name: name.into(),
            interface_type: InterfaceType::Wifi,
            mac: Default::default(),
            state: InterfaceState::Disconnected,
            driver: String::new(),
        });
        self.caps.insert(
            name.into(),
            AdapterCapabilities {
                interface: name.into(),
                ..caps
            },
        );
        self
    }

    /// Marks `name` as joined to a network on `frequency`, which also makes it
    /// the machine's upstream.
    pub fn joined_to(mut self, name: &str, frequency: u32) -> Self {
        if let Some(dev) = self.devices.iter_mut().find(|d| d.name == name) {
            dev.state = InterfaceState::Up;
        }
        self.stations.insert(
            name.into(),
            StationConnection {
                interface: name.into(),
                ssid: Some("HomeNet".into()),
                frequency,
            },
        );
        self.upstream = Some(name.into());
        self
    }

    pub fn upstream(mut self, name: &str) -> Self {
        self.upstream = Some(name.into());
        self
    }

    pub fn failing_disconnects(mut self) -> Self {
        self.disconnect_fails = true;
        self
    }

    pub fn disconnected_macs(&self) -> Vec<MacAddress> {
        self.disconnected.lock().unwrap().clone()
    }
}

impl Default for FakeNm {
    fn default() -> Self {
        Self::new()
    }
}

/// Capabilities for a radio that supports AP mode, parameterised on how it
/// handles running an AP alongside a station connection.
pub fn caps(sta_ap: bool, same_channel: bool) -> AdapterCapabilities {
    AdapterCapabilities {
        interface: "wlan0".into(),
        phy_name: "phy0".into(),
        driver: "iwlwifi".into(),
        supports_ap: true,
        supports_wpa3: true,
        supports_wifi_6: false,
        supports_wifi_6e: false,
        supports_wifi_7: false,
        detected: true,
        supports_simultaneous_sta_ap: sta_ap,
        sta_ap_same_channel_only: same_channel,
        supported_bands: vec![Band::Band2_4Ghz, Band::Band5Ghz],
        supported_channels_2ghz: (1..=13).collect(),
        supported_channels_5ghz: vec![36, 40, 44, 48, 112, 149],
        max_sta: 32,
    }
}

/// A valid hotspot config to build test cases from.
pub fn config() -> HotspotConfig {
    HotspotConfig {
        ssid: "Nimbus".into(),
        password: "password123".into(),
        ..Default::default()
    }
}

/// A connected device that joined `seconds_ago`.
pub fn station(mac: [u8; 6], seconds_ago: i64) -> StationInfo {
    StationInfo {
        mac: MacAddress::new(mac),
        ip: None,
        hostname: None,
        manufacturer: None,
        signal_dbm: -50,
        signal_percent: 100,
        rx_bytes: 0,
        tx_bytes: 0,
        rx_rate_mbps: 0.0,
        tx_rate_mbps: 0.0,
        connected_since: chrono::Utc::now() - chrono::Duration::seconds(seconds_ago),
    }
}

#[async_trait::async_trait]
impl NetworkManagerApi for FakeNm {
    async fn get_wifi_devices(&self) -> Result<Vec<NetworkInterface>> {
        Ok(self.devices.clone())
    }

    async fn get_all_interfaces(&self) -> Result<Vec<NetworkInterface>> {
        Ok(self.devices.clone())
    }

    async fn get_adapter_capabilities(&self, iface: &str) -> Result<AdapterCapabilities> {
        self.caps
            .get(iface)
            .cloned()
            .ok_or_else(|| NimbusError::InterfaceNotFound(iface.into()))
    }

    async fn create_hotspot(&self, _: &HotspotConfig, _: &str) -> Result<HotspotInfo> {
        unreachable!("planning must not create anything")
    }

    async fn stop_hotspot(&self) -> Result<()> {
        unreachable!("planning must not stop anything")
    }

    async fn get_active_hotspot(&self) -> Result<Option<HotspotInfo>> {
        Ok(None)
    }

    async fn get_connected_stations(&self, _: &str) -> Result<Vec<StationInfo>> {
        Ok(self.connected.lock().unwrap().clone())
    }

    async fn disconnect_station(&self, _: &str, mac: &MacAddress) -> Result<()> {
        if self.disconnect_fails {
            return Err(NimbusError::IwError("Operation not permitted".into()));
        }
        self.disconnected.lock().unwrap().push(*mac);
        self.connected.lock().unwrap().retain(|s| s.mac != *mac);
        Ok(())
    }

    async fn get_upstream_interface(&self) -> Result<Option<String>> {
        Ok(self.upstream.clone())
    }

    async fn get_station_connection(&self, iface: &str) -> Result<Option<StationConnection>> {
        Ok(self.stations.get(iface).cloned())
    }

    async fn is_nm_available(&self) -> bool {
        true
    }
}
