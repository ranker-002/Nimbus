use std::process::Command;

use nimbus_core::error::{NimbusError, Result};

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

        let status = Command::new("nft")
            .args(["-f", "-"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                if let Some(ref mut stdin) = child.stdin {
                    std::io::Write::write_all(stdin, rules.as_bytes())?;
                }
                child.wait()
            })
            .map_err(|e| NimbusError::NftablesError(format!("Failed to run nft: {}", e)))?;

        if !status.success() {
            return Err(NimbusError::NftablesError("nft command failed".into()));
        }

        enable_ip_forward().await?;
        Ok(())
    }

    pub async fn cleanup(&self, _ap_iface: &str, _upstream_iface: &str) -> Result<()> {
        let _ = Command::new("nft")
            .args(["delete", "table", "ip", "nimbus"])
            .status();

        disable_ip_forward().await?;
        Ok(())
    }

    pub async fn add_blacklist_rule(&self, ap_iface: &str, mac: &str) -> Result<()> {
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

        let status = Command::new("nft")
            .args(["-f", "-"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                if let Some(ref mut stdin) = child.stdin {
                    std::io::Write::write_all(stdin, rule.as_bytes())?;
                }
                child.wait()
            })
            .map_err(|e| NimbusError::NftablesError(format!("Failed to run nft: {}", e)))?;

        if !status.success() {
            return Err(NimbusError::NftablesError(
                "Failed to add blacklist rule".into(),
            ));
        }
        Ok(())
    }

    pub async fn remove_blacklist_rule(&self, _ap_iface: &str, _mac: &str) -> Result<()> {
        let _ = Command::new("nft")
            .args([
                "delete", "rule", "ip", "nimbus", "blacklist", "handle", "0",
            ])
            .status();
        Ok(())
    }

    pub async fn enable_client_isolation(&self, ap_iface: &str) -> Result<()> {
        let _ = Command::new("nft")
            .args([
                "add", "rule", "ip", "nimbus", "forward", "iifname", ap_iface, "oifname",
                ap_iface, "drop",
            ])
            .status();
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
