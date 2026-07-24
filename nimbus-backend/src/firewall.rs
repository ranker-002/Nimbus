use nimbus_core::error::{NimbusError, Result};
use nimbus_network::manager::validate_mac;

pub struct FirewallManager;

impl Default for FirewallManager {
    fn default() -> Self {
        Self::new()
    }
}

impl FirewallManager {
    pub fn new() -> Self {
        Self
    }

    pub async fn setup_nat(&self, ap_iface: &str, upstream_iface: &str) -> Result<()> {
        if upstream_iface.is_empty() {
            return Err(NimbusError::NftablesError(
                "No upstream interface for NAT".into(),
            ));
        }

        let rules = format!(
            r#"
            table ip nimbus {{
                chain postrouting {{
                    type nat hook postrouting priority 100; policy accept;
                    ip saddr 10.42.0.0/24 oifname "{upstream}" masquerade
                }}
                chain forward {{
                    type filter hook forward priority 0; policy drop;
                    iifname "{ap}" oifname "{upstream}" ct state new,established,related accept
                    iifname "{upstream}" oifname "{ap}" ct state established,related accept
                }}
            }}
            "#,
            ap = ap_iface,
            upstream = upstream_iface,
        );

        let mut child = tokio::process::Command::new("nft")
            .args(["-f", "-"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| NimbusError::NftablesError(format!("Failed to run nft: {}", e)))?;

        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            stdin.write_all(rules.as_bytes()).await.map_err(|e| {
                NimbusError::NftablesError(format!("Failed to write nft rules: {}", e))
            })?;
        }

        let status = child
            .wait()
            .await
            .map_err(|e| NimbusError::NftablesError(format!("Failed to wait nft: {}", e)))?;

        if !status.success() {
            return Err(NimbusError::NftablesError("nft command failed".into()));
        }

        enable_ip_forward().await?;
        Ok(())
    }

    pub async fn cleanup(&self, _ap_iface: &str, _upstream_iface: &str) -> Result<()> {
        let _ = tokio::process::Command::new("nft")
            .args(["delete", "table", "ip", "nimbus"])
            .status()
            .await;

        disable_ip_forward().await?;
        Ok(())
    }

    pub async fn add_blacklist_rule(&self, ap_iface: &str, mac: &str) -> Result<()> {
        if !validate_mac(mac) {
            return Err(NimbusError::InvalidValue(format!(
                "Invalid MAC address: {}",
                mac
            )));
        }

        let rule = format!(
            r#"
            table ip nimbus {{
                chain blacklist {{
                    iifname "{ap}" ether saddr {mac} drop
                }}
            }}
            "#,
            ap = ap_iface,
            mac = mac,
        );

        let mut child = tokio::process::Command::new("nft")
            .args(["-f", "-"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| NimbusError::NftablesError(format!("Failed to run nft: {}", e)))?;

        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            stdin.write_all(rule.as_bytes()).await.map_err(|e| {
                NimbusError::NftablesError(format!("Failed to write nft rule: {}", e))
            })?;
        }

        let status = child
            .wait()
            .await
            .map_err(|e| NimbusError::NftablesError(format!("Failed to wait nft: {}", e)))?;

        if !status.success() {
            return Err(NimbusError::NftablesError(
                "Failed to add blacklist rule".into(),
            ));
        }
        Ok(())
    }

    pub async fn remove_blacklist_rule(&self, _ap_iface: &str, mac: &str) -> Result<()> {
        if !validate_mac(mac) {
            return Err(NimbusError::InvalidValue(format!(
                "Invalid MAC address: {}",
                mac
            )));
        }

        let output = tokio::process::Command::new("nft")
            .args(["-a", "list", "chain", "ip", "nimbus", "blacklist"])
            .output()
            .await
            .map_err(|e| NimbusError::NftablesError(format!("Failed to list rules: {}", e)))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if line.contains(mac) {
                if let Some(handle_pos) = line.rfind("handle ") {
                    let handle_str = &line[handle_pos + 8..].trim();
                    if let Ok(handle) = handle_str.parse::<u32>() {
                        let _ = tokio::process::Command::new("nft")
                            .args([
                                "delete",
                                "rule",
                                "ip",
                                "nimbus",
                                "blacklist",
                                "handle",
                                &handle.to_string(),
                            ])
                            .status()
                            .await;
                        return Ok(());
                    }
                }
            }
        }

        Err(NimbusError::NftablesError(format!(
            "Blacklist rule for {} not found",
            mac
        )))
    }

    pub async fn enable_client_isolation(&self, ap_iface: &str) -> Result<()> {
        let status = tokio::process::Command::new("nft")
            .args([
                "add", "rule", "ip", "nimbus", "forward", "iifname", ap_iface, "oifname",
                ap_iface, "drop",
            ])
            .status()
            .await
            .map_err(|e| NimbusError::NftablesError(format!("Failed to run nft: {}", e)))?;

        if !status.success() {
            return Err(NimbusError::NftablesError(
                "Failed to enable client isolation".into(),
            ));
        }
        Ok(())
    }
}

async fn enable_ip_forward() -> Result<()> {
    tokio::fs::write("/proc/sys/net/ipv4/ip_forward", "1").await?;
    Ok(())
}

async fn disable_ip_forward() -> Result<()> {
    let _ = tokio::fs::write("/proc/sys/net/ipv4/ip_forward", "0").await;
    Ok(())
}
