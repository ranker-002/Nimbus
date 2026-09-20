use std::sync::atomic::{AtomicBool, Ordering};

use nimbus_core::error::{NimbusError, Result};
use nimbus_network::manager::validate_mac;

const IP_FORWARD_PATH: &str = "/proc/sys/net/ipv4/ip_forward";

#[derive(Default)]
pub struct FirewallManager {
    /// Whether IP forwarding was off before we turned it on. Only then may
    /// cleanup turn it back off — other software on this machine (containers,
    /// VMs, VPNs) may be relying on it.
    enabled_ip_forward: AtomicBool,
}

impl FirewallManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn setup_nat(&self, ap_iface: &str, upstream_iface: &str) -> Result<()> {
        if upstream_iface.is_empty() {
            return Err(NimbusError::NftablesError(
                "No upstream interface for NAT".into(),
            ));
        }

        // Both chains use `policy accept`. A base chain with `policy drop` at
        // the forward hook applies to the whole machine, not just our traffic:
        // nftables evaluates every base chain registered on a hook and a single
        // drop wins, so it would silently break Docker, VMs and any other
        // routing on the host for as long as the hotspot is up.
        let rules = format!(
            r#"
            table ip nimbus {{
                chain postrouting {{
                    type nat hook postrouting priority 100; policy accept;
                    ip saddr {subnet} oifname "{upstream}" masquerade
                }}
                chain forward {{
                    type filter hook forward priority 0; policy accept;
                    iifname "{ap}" oifname "{upstream}" ct state new,established,related accept
                    iifname "{upstream}" oifname "{ap}" ct state established,related accept
                }}
            }}
            "#,
            ap = ap_iface,
            upstream = upstream_iface,
            // Kept in step with the range dnsmasq hands out.
            subnet = crate::dhcp::SUBNET_CIDR,
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

        if !read_ip_forward().await {
            write_ip_forward(true).await?;
            self.enabled_ip_forward.store(true, Ordering::SeqCst);
        }
        Ok(())
    }

    pub async fn cleanup(&self, _ap_iface: &str, _upstream_iface: &str) -> Result<()> {
        // Capture the output rather than letting nft write to our stderr:
        // deleting a table that was never created is the normal case on a
        // failed start, and "No such file or directory" printed raw to the
        // terminal reads like something went badly wrong.
        match tokio::process::Command::new("nft")
            .args(["delete", "table", "ip", "nimbus"])
            .output()
            .await
        {
            Ok(output) if output.status.success() => log::debug!("Removed the nimbus nft table"),
            Ok(output) => log::debug!(
                "No nimbus nft table to remove: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
            Err(e) => log::warn!("Could not run nft: {}", e),
        }

        // Only undo forwarding if we are the ones who switched it on.
        if self.enabled_ip_forward.swap(false, Ordering::SeqCst) {
            let _ = write_ip_forward(false).await;
        }
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
                        // Captured, not inherited: nft must not write to the
                        // application's stderr.
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
                            .output()
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
        let output = tokio::process::Command::new("nft")
            .args([
                "add", "rule", "ip", "nimbus", "forward", "iifname", ap_iface, "oifname", ap_iface,
                "drop",
            ])
            .output()
            .await
            .map_err(|e| NimbusError::NftablesError(format!("Failed to run nft: {}", e)))?;

        if !output.status.success() {
            return Err(NimbusError::NftablesError(format!(
                "Failed to enable client isolation: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(())
    }
}

async fn read_ip_forward() -> bool {
    tokio::fs::read_to_string(IP_FORWARD_PATH)
        .await
        .map(|v| v.trim() == "1")
        .unwrap_or(false)
}

async fn write_ip_forward(enabled: bool) -> Result<()> {
    tokio::fs::write(IP_FORWARD_PATH, if enabled { "1" } else { "0" }).await?;
    Ok(())
}
