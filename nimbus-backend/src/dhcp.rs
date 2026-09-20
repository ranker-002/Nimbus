//! Address handout for hotspot clients, via dnsmasq.
//!
//! Devices that associate with the access point need an address, a gateway and
//! a resolver before they can use the shared connection. hostapd does not do
//! any of that — it only handles the radio link — so a DHCP server has to run
//! alongside it on the AP interface.
//!
//! dnsmasq is the same dependency NetworkManager's own `ipv4.method=shared`
//! pulls in, and what `create_ap` and friends use.

use std::net::Ipv4Addr;
use std::process::Stdio;

use tokio::process::{Child, Command};

use nimbus_core::error::{NimbusError, Result};

/// The subnet the hotspot serves. Matches what NetworkManager uses for shared
/// connections, so behaviour is the same whichever backend runs.
pub const GATEWAY: Ipv4Addr = Ipv4Addr::new(10, 42, 0, 1);
pub const NETMASK: Ipv4Addr = Ipv4Addr::new(255, 255, 255, 0);
pub const POOL_START: Ipv4Addr = Ipv4Addr::new(10, 42, 0, 10);
pub const POOL_END: Ipv4Addr = Ipv4Addr::new(10, 42, 0, 254);
/// Gateway address in CIDR form, for assigning to the AP interface.
pub const GATEWAY_CIDR: &str = "10.42.0.1/24";
/// The subnet in CIDR form, for NAT rules.
pub const SUBNET_CIDR: &str = "10.42.0.0/24";

/// Command-line arguments for a dnsmasq bound to `interface`.
///
/// Everything is passed as arguments rather than a config file so that no
/// temporary file has to be written, and so `--conf-file=/dev/null` can keep
/// the system-wide dnsmasq configuration from leaking in.
pub fn build_args(interface: &str, lease_seconds: u32) -> Vec<String> {
    vec![
        // Ignore /etc/dnsmasq.conf: this instance serves only the hotspot.
        "--conf-file=/dev/null".to_string(),
        "--keep-in-foreground".to_string(),
        "--no-daemon".to_string(),
        // Never touch any interface but ours; without this dnsmasq would bind
        // the wildcard address and clash with a resolver already on :53.
        format!("--interface={}", interface),
        "--bind-interfaces".to_string(),
        "--except-interface=lo".to_string(),
        "--no-hosts".to_string(),
        format!("--listen-address={}", GATEWAY),
        format!(
            "--dhcp-range={},{},{},{}s",
            POOL_START, POOL_END, NETMASK, lease_seconds
        ),
        format!("--dhcp-option=option:router,{}", GATEWAY),
        format!("--dhcp-option=option:dns-server,{}", GATEWAY),
        // Authoritative: hand out an address immediately rather than waiting
        // for another server that will never answer on this subnet.
        "--dhcp-authoritative".to_string(),
    ]
}

/// Whether dnsmasq is installed.
pub async fn is_available() -> bool {
    Command::new("dnsmasq")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .is_ok()
}

/// A running dnsmasq serving the hotspot subnet.
pub struct DhcpServer {
    child: Child,
}

impl DhcpServer {
    pub async fn start(interface: &str, lease_seconds: u32) -> Result<Self> {
        let child = Command::new("dnsmasq")
            .args(build_args(interface, lease_seconds))
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                NimbusError::ConfigError(format!(
                    "Could not start dnsmasq: {}. It is required to give \
                     connected devices an IP address.",
                    e
                ))
            })?;

        // dnsmasq fails fast on a port clash or a bad range.
        tokio::time::sleep(std::time::Duration::from_millis(700)).await;
        let mut server = Self { child };
        if let Some(status) = server.child.try_wait()? {
            let stderr = server.take_stderr().await;
            return Err(NimbusError::ConfigError(format!(
                "dnsmasq exited immediately ({}): {}",
                status,
                stderr.trim()
            )));
        }
        Ok(server)
    }

    async fn take_stderr(&mut self) -> String {
        use tokio::io::AsyncReadExt;
        let Some(mut stderr) = self.child.stderr.take() else {
            return String::new();
        };
        let mut buffer = String::new();
        let _ = stderr.read_to_string(&mut buffer).await;
        buffer
    }

    pub async fn stop(mut self) {
        if let Err(e) = self.child.kill().await {
            log::warn!("Could not stop dnsmasq: {}", e);
        }
        let _ = self.child.wait().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args() -> Vec<String> {
        build_args("nimbus-ap", 3600)
    }

    #[test]
    fn serves_only_the_ap_interface() {
        let args = args();
        assert!(args.contains(&"--interface=nimbus-ap".to_string()));
        // Without binding to the one interface, dnsmasq grabs the wildcard
        // address and collides with any resolver already listening on :53.
        assert!(args.contains(&"--bind-interfaces".to_string()));
        assert!(args.contains(&"--except-interface=lo".to_string()));
    }

    #[test]
    fn ignores_the_system_configuration() {
        assert!(args().contains(&"--conf-file=/dev/null".to_string()));
    }

    #[test]
    fn hands_out_the_expected_range_and_lease() {
        assert!(args().contains(&format!(
            "--dhcp-range={},{},{},3600s",
            POOL_START, POOL_END, NETMASK
        )));
    }

    #[test]
    fn advertises_the_gateway_as_router_and_resolver() {
        let args = args();
        assert!(args.contains(&"--dhcp-option=option:router,10.42.0.1".to_string()));
        assert!(args.contains(&"--dhcp-option=option:dns-server,10.42.0.1".to_string()));
    }

    #[test]
    fn pool_stays_inside_the_subnet_and_skips_the_gateway() {
        assert!(POOL_START > GATEWAY);
        assert!(POOL_START < POOL_END);
        assert_eq!(GATEWAY.octets()[..3], POOL_START.octets()[..3]);
        assert_eq!(GATEWAY.octets()[..3], POOL_END.octets()[..3]);
        // .255 is the broadcast address and must stay out of the pool.
        assert!(POOL_END.octets()[3] < 255);
    }

    #[test]
    fn gateway_cidr_matches_the_gateway_address() {
        assert!(GATEWAY_CIDR.starts_with(&GATEWAY.to_string()));
    }

    #[test]
    fn runs_in_the_foreground_so_it_can_be_supervised() {
        let args = args();
        assert!(args.contains(&"--keep-in-foreground".to_string()));
        assert!(args.contains(&"--no-daemon".to_string()));
    }
}
