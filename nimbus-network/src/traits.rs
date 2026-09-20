use async_trait::async_trait;
use mac_address::MacAddress;
use nimbus_core::types::{
    AdapterCapabilities, HotspotConfig, HotspotInfo, NetworkInterface, StationConnection,
    StationInfo,
};
use nimbus_core::Result;

#[async_trait]
pub trait NetworkManagerApi: Send + Sync {
    async fn get_wifi_devices(&self) -> Result<Vec<NetworkInterface>>;

    async fn get_all_interfaces(&self) -> Result<Vec<NetworkInterface>>;

    async fn get_adapter_capabilities(&self, interface: &str) -> Result<AdapterCapabilities>;

    async fn create_hotspot(&self, config: &HotspotConfig, interface: &str) -> Result<HotspotInfo>;

    async fn stop_hotspot(&self) -> Result<()>;

    async fn get_active_hotspot(&self) -> Result<Option<HotspotInfo>>;

    async fn get_connected_stations(&self, interface: &str) -> Result<Vec<StationInfo>>;

    /// Disassociates a station from the hotspot running on `interface`.
    ///
    /// Used to hold the connected-device count at the configured limit. The
    /// device is free to try again; enforcement re-runs on the next poll.
    async fn disconnect_station(&self, interface: &str, mac: &MacAddress) -> Result<()>;

    async fn get_upstream_interface(&self) -> Result<Option<String>>;

    /// The client Wi-Fi connection `interface` currently holds, if it is joined
    /// to a network. Used to work out whether bringing up an AP on that same
    /// radio would tear the connection down.
    async fn get_station_connection(&self, interface: &str) -> Result<Option<StationConnection>>;

    async fn is_nm_available(&self) -> bool;
}
