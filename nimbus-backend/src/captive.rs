use nimbus_core::error::{NimbusError, Result};

pub struct CaptivePortalManager;

impl Default for CaptivePortalManager {
    fn default() -> Self {
        Self::new()
    }
}

impl CaptivePortalManager {
    pub fn new() -> Self {
        Self
    }

    pub async fn setup(&self, ap_iface: &str, portal_ip: &str) -> Result<()> {
        let dns_hijack = format!(
            r#"
address=/#/{portal_ip}
address=/captive.apple.com/{portal_ip}
address=/connectivitycheck.gstatic.com/{portal_ip}
address=/www.msftconnecttest.com/{portal_ip}
address=/detectportal.firefox.com/{portal_ip}
address=/nmcheck.gnome.org/{portal_ip}
"#,
            portal_ip = portal_ip,
        );

        let config_path = "/etc/NetworkManager/dnsmasq-shared.d/nimbus-captive.conf";
        tokio::fs::write(config_path, &dns_hijack).await.map_err(|e| {
            NimbusError::ConfigError(format!("Failed to write captive DNS: {}", e))
        })?;

        let nft_rule = format!(
            r#"
            table ip nimbus {{
                chain captive {{
                    type nat hook prerouting priority -100; policy accept;
                    iifname "{iface}" tcp dport 80 redirect to :8080
                }}
            }}
            "#,
            iface = ap_iface,
        );

        let mut child = tokio::process::Command::new("nft")
            .args(["-f", "-"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| NimbusError::NftablesError(format!("Failed to run nft: {}", e)))?;

        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            stdin.write_all(nft_rule.as_bytes()).await.map_err(|e| {
                NimbusError::NftablesError(format!("Failed to write nft rule: {}", e))
            })?;
        }

        let status = child
            .wait()
            .await
            .map_err(|e| NimbusError::NftablesError(format!("Failed to wait nft: {}", e)))?;

        if !status.success() {
            return Err(NimbusError::NftablesError(
                "nft captive portal rule failed".into(),
            ));
        }

        Ok(())
    }

    pub async fn cleanup(&self) -> Result<()> {
        let config_path = "/etc/NetworkManager/dnsmasq-shared.d/nimbus-captive.conf";
        match tokio::fs::remove_file(config_path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(NimbusError::ConfigError(format!(
                "Failed to remove captive DNS config: {}",
                e
            ))),
        }
    }
}
