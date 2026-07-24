use nimbus_core::error::Result;
use nimbus_core::types::HotspotConfig;

use crate::manager::NmManager;
use crate::traits::NetworkManagerApi;

pub struct HotspotManager {
    nm: NmManager,
}

impl HotspotManager {
    pub fn new(nm: NmManager) -> Self {
        Self { nm }
    }

    pub async fn start(&self, config: &HotspotConfig, interface: &str) -> Result<()> {
        config.validate()?;

        if !self.nm.is_nm_available().await {
            return Err(nimbus_core::NimbusError::NetworkManagerUnavailable(
                "NetworkManager D-Bus service not reachable".into(),
            ));
        }

        let caps = self.nm.get_adapter_capabilities(interface).await?;

        if !caps.supports_ap {
            return Err(nimbus_core::NimbusError::ApModeNotSupported(
                interface.to_string(),
            ));
        }

        if config.security == nimbus_core::types::Security::Wpa3 && !caps.supports_wpa3 {
            return Err(nimbus_core::NimbusError::Wpa3NotSupported(
                interface.to_string(),
            ));
        }

        if config.band == nimbus_core::types::Band::Band5Ghz && caps.supported_channels_5ghz.is_empty() {
            return Err(nimbus_core::NimbusError::Band5GhzNotSupported(
                interface.to_string(),
            ));
        }

        self.nm.create_hotspot(config, interface).await?;
        Ok(())
    }

    pub async fn stop(&self) -> Result<()> {
        self.nm.stop_hotspot().await
    }

    pub async fn get_status(&self) -> Result<Option<nimbus_core::types::HotspotInfo>> {
        self.nm.get_active_hotspot().await
    }
}
