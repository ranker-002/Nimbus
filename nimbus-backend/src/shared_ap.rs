//! The backend that shares the machine's own Wi-Fi connection.
//!
//! Brings up an access point on a dedicated virtual interface and routes its
//! clients out through whatever connection the machine is already using, so the
//! station link is never touched:
//!
//! ```text
//!   phone ──wifi──▶ nimbus-ap (10.42.0.1)  ──NAT──▶ wlp4s0 ──▶ router
//!                   hostapd + dnsmasq                 (unchanged)
//! ```
//!
//! Both interfaces live on the same radio, so the AP is pinned to the channel
//! the station connection already uses — most radios only allow the pair on one
//! channel (`#channels <= 1` in `iw phy info`).
//!
//! Everything here needs `CAP_NET_ADMIN`.

use nimbus_core::error::{NimbusError, Result};
use nimbus_core::types::{HotspotConfig, HotspotInfo};
use nimbus_network::virtual_ap;

use crate::dhcp::{self, DhcpServer};
use crate::firewall::FirewallManager;
use crate::hostapd::{self, Hostapd};

/// How long clients keep an address before renewing.
const LEASE_SECONDS: u32 = 3600;

/// A running shared access point, and everything that has to be undone to
/// remove it.
pub struct SharedAp {
    hostapd: Option<Hostapd>,
    dhcp: Option<DhcpServer>,
    firewall: FirewallManager,
    ap_interface: String,
    upstream: String,
}

/// What the machine is missing before this backend can be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Missing {
    Root,
    Hostapd,
    Dnsmasq,
    /// Only blocks when a country has to be applied, or when the target
    /// channel is one the world domain forbids transmitting on.
    RegulatoryDatabase,
}

impl Missing {
    /// A message naming the fix, rather than just the problem.
    pub fn explain(&self) -> &'static str {
        match self {
            Missing::Root => {
                "Sharing your Wi-Fi connection needs administrator rights; run \
                 Nimbus with sudo."
            }
            Missing::Hostapd => {
                "hostapd is not installed. It runs the access point that lets \
                 you stay connected while sharing."
            }
            Missing::Dnsmasq => {
                "dnsmasq is not installed. It gives connected devices their IP \
                 address; without it they can join the hotspot but get no \
                 network."
            }
            Missing::RegulatoryDatabase => {
                "the wireless-regdb package is not installed, so the machine is \
                 stuck on the world regulatory domain, where 5 GHz is \
                 receive-only and no access point may transmit."
            }
        }
    }
}

/// Reports everything standing in the way of sharing the connection.
pub async fn check_requirements() -> Vec<Missing> {
    let mut missing = Vec::new();
    if !is_root() {
        missing.push(Missing::Root);
    }
    if !hostapd::is_available().await {
        missing.push(Missing::Hostapd);
    }
    if !dhcp::is_available().await {
        missing.push(Missing::Dnsmasq);
    }
    missing
}

fn is_root() -> bool {
    // Safe: geteuid cannot fail and touches no memory we own.
    unsafe { libc::geteuid() == 0 }
}

/// Removes a shared access point left behind by a previous run.
///
/// A crash — or hostapd dying after the rest was set up — leaves the virtual
/// interface, a dnsmasq and the NAT table in place with no owner. Nothing will
/// ever tear them down, and the stray interface confuses the next start, so
/// this runs at startup before any state is reported.
///
/// Returns whether anything was cleaned up. Requires root; a no-op otherwise.
pub async fn cleanup_orphans() -> bool {
    if !is_root() || !virtual_ap::exists(virtual_ap::AP_INTERFACE).await {
        return false;
    }

    log::warn!(
        "Found a leftover {} from an earlier run; removing it",
        virtual_ap::AP_INTERFACE
    );

    // Matched on the interface argument so only our own helpers are hit, never
    // a dnsmasq or hostapd the rest of the system is relying on.
    for pattern in [
        format!("dnsmasq.*--interface={}", virtual_ap::AP_INTERFACE),
        format!("hostapd.*{}", virtual_ap::AP_INTERFACE),
    ] {
        let _ = tokio::process::Command::new("pkill")
            .args(["-f", &pattern])
            .output()
            .await;
    }

    let firewall = FirewallManager::new();
    let _ = firewall.cleanup(virtual_ap::AP_INTERFACE, "").await;

    if let Err(e) = virtual_ap::destroy().await {
        log::warn!("Could not remove {}: {}", virtual_ap::AP_INTERFACE, e);
        return false;
    }
    true
}

impl SharedAp {
    /// Brings the shared access point up.
    ///
    /// `base_interface` is the adapter holding the station connection,
    /// `channel` the channel it is on, and `upstream` where client traffic
    /// should be routed. Anything already created is torn down on failure, so a
    /// failed start leaves nothing behind.
    pub async fn start(
        config: &HotspotConfig,
        base_interface: &str,
        base_mac: mac_address::MacAddress,
        channel: u32,
        upstream: &str,
    ) -> Result<Self> {
        let missing = check_requirements().await;
        if let Some(first) = missing.first() {
            return Err(NimbusError::ConfigError(first.explain().to_string()));
        }

        let ap_interface = virtual_ap::create(base_interface, base_mac).await?;

        let mut ap = Self {
            hostapd: None,
            dhcp: None,
            firewall: FirewallManager::new(),
            ap_interface: ap_interface.clone(),
            upstream: upstream.to_string(),
        };

        if let Err(e) = ap.bring_up(config, channel).await {
            ap.shutdown().await;
            return Err(e);
        }

        Ok(ap)
    }

    async fn bring_up(&mut self, config: &HotspotConfig, channel: u32) -> Result<()> {
        let text = hostapd::build_config(config, &self.ap_interface, channel);
        let dfs = nimbus_core::types::is_dfs_channel(channel);
        self.hostapd = Some(Hostapd::start(&text, dfs).await?);

        virtual_ap::assign_address(&self.ap_interface, dhcp::GATEWAY_CIDR).await?;
        self.dhcp = Some(DhcpServer::start(&self.ap_interface, LEASE_SECONDS).await?);

        // Without this, clients associate and get an address but cannot reach
        // anything beyond the gateway.
        self.firewall
            .setup_nat(&self.ap_interface, &self.upstream)
            .await?;

        Ok(())
    }

    pub fn interface(&self) -> &str {
        &self.ap_interface
    }

    pub fn info(&self, config: &HotspotConfig, channel: u32) -> HotspotInfo {
        HotspotInfo {
            interface: self.ap_interface.clone(),
            ssid: config.ssid.clone(),
            ip: dhcp::GATEWAY,
            frequency: nimbus_core::types::channel_to_freq(channel),
            channel,
            started_at: std::time::Instant::now(),
        }
    }

    /// Tears everything down, in reverse order. Best-effort throughout: one
    /// failing step must not strand the others.
    pub async fn shutdown(mut self) {
        self.teardown().await;
    }

    async fn teardown(&mut self) {
        if let Some(dhcp) = self.dhcp.take() {
            dhcp.stop().await;
        }
        if let Some(hostapd) = self.hostapd.take() {
            hostapd.stop().await;
        }
        if let Err(e) = self
            .firewall
            .cleanup(&self.ap_interface, &self.upstream)
            .await
        {
            log::warn!("Firewall cleanup failed: {}", e);
        }
        if let Err(e) = virtual_ap::destroy().await {
            log::warn!("Could not remove {}: {}", self.ap_interface, e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_missing_requirement_names_a_fix() {
        for missing in [
            Missing::Root,
            Missing::Hostapd,
            Missing::Dnsmasq,
            Missing::RegulatoryDatabase,
        ] {
            let text = missing.explain();
            assert!(!text.is_empty());
            // Each message should tell the user what to do, not just what failed.
            assert!(
                text.contains("sudo") || text.contains("not installed"),
                "{:?} does not say how to fix it",
                missing
            );
        }
    }

    #[tokio::test]
    async fn requirements_are_reported_rather_than_assumed() {
        // Whatever this machine has, the check must return without panicking
        // and only ever report known blockers.
        for missing in check_requirements().await {
            assert!(matches!(
                missing,
                Missing::Root | Missing::Hostapd | Missing::Dnsmasq | Missing::RegulatoryDatabase
            ));
        }
    }

    #[tokio::test]
    async fn a_non_root_run_is_always_blocked() {
        if !is_root() {
            assert!(check_requirements().await.contains(&Missing::Root));
        }
    }
}
