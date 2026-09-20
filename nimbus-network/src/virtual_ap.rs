//! A second, AP-mode network device carved out of an existing Wi-Fi radio.
//!
//! This is what lets the machine keep its own Wi-Fi connection while running a
//! hotspot. NetworkManager activates an access point by switching the *existing*
//! device into AP mode, which necessarily drops the station connection, because
//! a device holds one active connection at a time.
//!
//! Most radios can host both at once, but only across two separate netdevs —
//! `iw phy` reports it as `#{ managed } <= 2, #{ AP } <= 1`. So Nimbus asks the
//! driver for a dedicated AP interface and runs hostapd on that, leaving the
//! original device untouched.
//!
//! The usual constraint is `#channels <= 1`: both interfaces must sit on the
//! same channel, so the AP has to follow whatever channel the station
//! connection is already using.

use mac_address::MacAddress;
use tokio::process::Command;

use nimbus_core::error::{NimbusError, Result};

/// Name given to the interface Nimbus creates. Fixed, so a leftover from a
/// crashed run is recognisable and can be cleaned up.
pub const AP_INTERFACE: &str = "nimbus-ap";

/// Derives the MAC for the AP interface from the base radio's MAC.
///
/// The two interfaces share a radio but must not share a MAC, or the driver
/// refuses to bring the second one up. Setting the locally-administered bit
/// yields an address that cannot collide with a real vendor-assigned one.
pub fn derive_ap_mac(base: MacAddress) -> MacAddress {
    let mut bytes = base.bytes();
    // Locally administered, and definitely not multicast.
    bytes[0] = (bytes[0] | 0x02) & 0xFE;

    // If the base was already locally administered the two would be identical,
    // so perturb another octet to keep them distinct.
    if bytes == base.bytes() {
        bytes[5] ^= 0x01;
    }
    MacAddress::new(bytes)
}

/// Whether the AP interface currently exists.
pub async fn exists(interface: &str) -> bool {
    tokio::fs::metadata(format!("/sys/class/net/{}", interface))
        .await
        .is_ok()
}

/// Creates the AP interface on the same radio as `base_interface` and brings it
/// up with its own MAC.
///
/// Requires `CAP_NET_ADMIN`. Any interface left over from an earlier run is
/// removed first so the name is always free.
pub async fn create(base_interface: &str, base_mac: MacAddress) -> Result<String> {
    if exists(AP_INTERFACE).await {
        log::info!("Removing leftover {} before recreating it", AP_INTERFACE);
        destroy().await?;
    }

    run(
        "iw",
        &[
            "dev",
            base_interface,
            "interface",
            "add",
            AP_INTERFACE,
            "type",
            "__ap",
        ],
    )
    .await
    .map_err(|e| {
        NimbusError::IwError(format!(
            "Could not create an AP interface on {}: {}. The adapter may not \
             support running an access point alongside a connection.",
            base_interface, e
        ))
    })?;

    // The MAC has to be set while the link is down.
    let mac = derive_ap_mac(base_mac).to_string().to_lowercase();
    if let Err(e) = run("ip", &["link", "set", "dev", AP_INTERFACE, "down"]).await {
        log::warn!(
            "Could not take {} down before setting its MAC: {}",
            AP_INTERFACE,
            e
        );
    }
    if let Err(e) = run("ip", &["link", "set", "dev", AP_INTERFACE, "address", &mac]).await {
        // Not fatal on every driver, but usually means the AP will not come up.
        log::warn!(
            "Could not set the MAC of {} to {}: {}",
            AP_INTERFACE,
            mac,
            e
        );
    }

    run("ip", &["link", "set", "dev", AP_INTERFACE, "up"]).await?;

    // NetworkManager would otherwise try to manage the new device and fight
    // hostapd for it.
    if let Err(e) = set_nm_managed(AP_INTERFACE, false).await {
        log::warn!(
            "Could not unmanage {} in NetworkManager: {}",
            AP_INTERFACE,
            e
        );
    }

    Ok(AP_INTERFACE.to_string())
}

/// Removes the AP interface. Safe to call when it does not exist.
pub async fn destroy() -> Result<()> {
    if !exists(AP_INTERFACE).await {
        return Ok(());
    }

    // Hand the name back to NetworkManager before deleting, so a future device
    // with the same name is not left permanently unmanaged.
    let _ = set_nm_managed(AP_INTERFACE, true).await;
    let _ = run("ip", &["link", "set", "dev", AP_INTERFACE, "down"]).await;
    run("iw", &["dev", AP_INTERFACE, "del"]).await
}

/// Assigns the gateway address the hotspot serves from.
pub async fn assign_address(interface: &str, cidr: &str) -> Result<()> {
    // Clear anything stale so repeated starts do not stack addresses.
    let _ = run("ip", &["addr", "flush", "dev", interface]).await;
    run("ip", &["addr", "add", cidr, "dev", interface]).await
}

async fn set_nm_managed(interface: &str, managed: bool) -> Result<()> {
    run(
        "nmcli",
        &[
            "device",
            "set",
            interface,
            "managed",
            if managed { "yes" } else { "no" },
        ],
    )
    .await
}

async fn run(program: &str, args: &[&str]) -> Result<()> {
    let output = Command::new(program)
        .args(args)
        .output()
        .await
        .map_err(|e| NimbusError::IwError(format!("Failed to run {}: {}", program, e)))?;

    if output.status.success() {
        return Ok(());
    }
    Err(NimbusError::IwError(format!(
        "{} {} failed: {}",
        program,
        args.join(" "),
        String::from_utf8_lossy(&output.stderr).trim()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_mac_sets_the_locally_administered_bit() {
        // A real Intel address, as reported by this machine.
        let base = MacAddress::new([0x14, 0x13, 0x33, 0x37, 0x4b, 0x89]);
        let derived = derive_ap_mac(base);
        assert_eq!(
            derived,
            MacAddress::new([0x16, 0x13, 0x33, 0x37, 0x4b, 0x89])
        );
        assert_eq!(derived.bytes()[0] & 0x02, 0x02);
    }

    #[test]
    fn derived_mac_is_never_multicast() {
        // An odd first octet marks a multicast address, which is invalid here.
        let base = MacAddress::new([0x01, 0x02, 0x03, 0x04, 0x05, 0x06]);
        assert_eq!(derive_ap_mac(base).bytes()[0] & 0x01, 0);
    }

    #[test]
    fn derived_mac_differs_from_an_already_local_base() {
        // Randomised MACs already have the local bit set; the derivation must
        // still produce a distinct address or the driver rejects the interface.
        let base = MacAddress::new([0x02, 0xaa, 0xbb, 0xcc, 0xdd, 0xee]);
        let derived = derive_ap_mac(base);
        assert_ne!(derived, base);
        assert_eq!(
            derived,
            MacAddress::new([0x02, 0xaa, 0xbb, 0xcc, 0xdd, 0xef])
        );
    }

    #[test]
    fn derivation_is_stable() {
        let base = MacAddress::new([0x14, 0x13, 0x33, 0x37, 0x4b, 0x89]);
        assert_eq!(derive_ap_mac(base), derive_ap_mac(base));
    }

    #[tokio::test]
    async fn a_missing_interface_does_not_exist() {
        assert!(!exists("nimbus-definitely-not-here").await);
    }

    #[tokio::test]
    async fn loopback_exists() {
        assert!(exists("lo").await);
    }
}
