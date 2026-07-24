
use nimbus_core::error::{NimbusError, Result};

pub struct DnsManager;

impl Default for DnsManager {
    fn default() -> Self {
        Self::new()
    }
}

impl DnsManager {
    pub fn new() -> Self {
        Self
    }

    pub async fn configure_shared(&self, interface: &str) -> Result<()> {
        let config = format!(
            r#"
interface={iface}
listen-address=10.42.0.1
bind-interfaces
dhcp-range=10.42.0.100,10.42.0.200,255.255.255.0,12h
dhcp-option=3,10.42.0.1
dhcp-option=6,10.42.0.1,1.1.1.1
dhcp-authoritative
"#,
            iface = interface,
        );

        let config_path = "/etc/NetworkManager/dnsmasq-shared.d/nimbus-hotspot.conf";
        std::fs::write(config_path, &config)
            .map_err(|e| NimbusError::ConfigError(format!("Failed to write dnsmasq config: {}", e)))?;

        Ok(())
    }

    pub async fn cleanup(&self) -> Result<()> {
        let config_path = "/etc/NetworkManager/dnsmasq-shared.d/nimbus-hotspot.conf";
        let _ = std::fs::remove_file(config_path);
        Ok(())
    }
}
