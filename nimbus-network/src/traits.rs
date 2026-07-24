use async_trait::async_trait;
use nimbus_core::types::{
    AdapterCapabilities, BandwidthSample, HotspotConfig, HotspotInfo, NetworkInterface, StationInfo,
};
use nimbus_core::Result;

#[async_trait]
pub trait NetworkManagerApi: Send + Sync {
    async fn get_wifi_devices(&self) -> Result<Vec<NetworkInterface>>;

    async fn get_all_interfaces(&self) -> Result<Vec<NetworkInterface>>;

    async fn get_adapter_capabilities(&self, interface: &str) -> Result<AdapterCapabilities>;

    async fn create_hotspot(
        &self,
        config: &HotspotConfig,
        interface: &str,
    ) -> Result<HotspotInfo>;

    async fn stop_hotspot(&self) -> Result<()>;

    async fn get_active_hotspot(&self) -> Result<Option<HotspotInfo>>;

    async fn get_connected_stations(&self, interface: &str) -> Result<Vec<StationInfo>>;

    async fn get_upstream_interface(&self) -> Result<Option<String>>;

    async fn get_bandwidth_sample(&self, interface: &str) -> Result<BandwidthSample>;

    async fn is_nm_available(&self) -> bool;
}
