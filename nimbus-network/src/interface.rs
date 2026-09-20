use nimbus_core::error::Result;
use nimbus_core::types::{InterfaceState, InterfaceType, NetworkInterface};

use mac_address::MacAddress;
use tokio::process::Command;

use crate::manager::parse_mac;

/// `nmcli device status` only knows these four fields; HWADDR is a
/// `device show` field and asking for it here makes nmcli fail outright.
const STATUS_FIELDS: &str = "DEVICE,TYPE,STATE,CONNECTION";

pub async fn detect_interfaces() -> Result<Vec<NetworkInterface>> {
    let output = Command::new("nmcli")
        .args(["-t", "-f", STATUS_FIELDS, "device", "status"])
        .output()
        .await?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut result = Vec::new();

    for line in stdout.lines() {
        let fields = split_terse(line);
        if fields.len() < 3 {
            continue;
        }
        let name = fields[0].clone();
        if name.is_empty() {
            continue;
        }

        result.push(NetworkInterface {
            interface_type: parse_type(&fields[1]),
            state: parse_state(&fields[2]),
            // nmcli cannot report the MAC here, so take it from sysfs.
            mac: read_mac(&name).await,
            driver: String::new(),
            name,
        });
    }

    Ok(result)
}

/// Split one line of `nmcli --terse` output. nmcli escapes `:` and `\` inside
/// field values with a backslash, so a plain `split(':')` mangles anything
/// containing a colon.
fn split_terse(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut chars = line.chars();

    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                // A backslash escapes the next character verbatim.
                if let Some(escaped) = chars.next() {
                    current.push(escaped);
                }
            }
            ':' => fields.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    fields.push(current);
    fields
}

fn parse_type(raw: &str) -> InterfaceType {
    match raw {
        "wifi" => InterfaceType::Wifi,
        "ethernet" => InterfaceType::Ethernet,
        "usb" | "gsm" | "cdma" => InterfaceType::UsbTethering,
        "bridge" => InterfaceType::Bridge,
        "wireguard" | "tun" | "vpn" => InterfaceType::Vpn,
        "modem" => InterfaceType::Modem,
        _ => InterfaceType::Unknown,
    }
}

/// nmcli decorates some states, e.g. `connected (externally)` or
/// `connected (site only)`, so match on the leading word.
fn parse_state(raw: &str) -> InterfaceState {
    match raw.split_whitespace().next().unwrap_or("") {
        "connected" => InterfaceState::Up,
        "disconnected" | "connecting" | "disconnecting" | "deactivating" => {
            InterfaceState::Disconnected
        }
        "unavailable" => InterfaceState::Unavailable,
        "unmanaged" => InterfaceState::Down,
        _ => InterfaceState::Disconnected,
    }
}

async fn read_mac(interface: &str) -> MacAddress {
    let path = format!("/sys/class/net/{}/address", interface);
    match tokio::fs::read_to_string(&path).await {
        Ok(raw) => parse_mac(raw.trim()),
        Err(_) => MacAddress::new([0; 6]),
    }
}

/// The interface currently carrying the default route, i.e. this machine's way
/// out to the internet. Falls back to nmcli's device list when no default route
/// exists yet.
pub async fn get_upstream_interface() -> Result<Option<String>> {
    if let Some(iface) = default_route_interface().await {
        return Ok(Some(iface));
    }

    let interfaces = detect_interfaces().await?;
    let priority = |t: &InterfaceType| match t {
        InterfaceType::Ethernet => 3,
        InterfaceType::Wifi => 2,
        InterfaceType::UsbTethering | InterfaceType::Modem => 1,
        _ => 0,
    };

    Ok(interfaces
        .iter()
        .filter(|i| i.state == InterfaceState::Up && priority(&i.interface_type) > 0)
        .max_by_key(|i| priority(&i.interface_type))
        .map(|i| i.name.clone()))
}

/// Reads the default route straight from `ip route`, which is what actually
/// determines where traffic leaves the machine.
async fn default_route_interface() -> Option<String> {
    let output = Command::new("ip")
        .args(["-4", "route", "show", "default"])
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    // e.g. "default via 192.168.1.254 dev wlp4s0 proto dhcp metric 600"
    for line in stdout.lines() {
        let mut tokens = line.split_whitespace();
        while let Some(token) = tokens.next() {
            if token == "dev" {
                return tokens.next().map(str::to_string);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_terse_plain() {
        let fields = split_terse("wlp4s0:wifi:connected:MyNetwork");
        assert_eq!(fields, ["wlp4s0", "wifi", "connected", "MyNetwork"]);
    }

    #[test]
    fn test_split_terse_unescapes_colons_in_values() {
        // A connection name containing a colon comes back escaped.
        let fields = split_terse(r"wlp4s0:wifi:connected:Cafe\: Free WiFi");
        assert_eq!(fields, ["wlp4s0", "wifi", "connected", "Cafe: Free WiFi"]);
    }

    #[test]
    fn test_split_terse_unescapes_backslash() {
        let fields = split_terse(r"eth0:ethernet:connected:Home\\Net");
        assert_eq!(fields, ["eth0", "ethernet", "connected", r"Home\Net"]);
    }

    #[test]
    fn test_split_terse_empty_trailing_field() {
        let fields = split_terse("enp5s0:ethernet:unavailable:");
        assert_eq!(fields, ["enp5s0", "ethernet", "unavailable", ""]);
    }

    #[test]
    fn test_parse_state_handles_decorated_connected() {
        assert_eq!(parse_state("connected"), InterfaceState::Up);
        assert_eq!(parse_state("connected (externally)"), InterfaceState::Up);
        assert_eq!(parse_state("connected (site only)"), InterfaceState::Up);
    }

    #[test]
    fn test_parse_state_variants() {
        assert_eq!(parse_state("disconnected"), InterfaceState::Disconnected);
        assert_eq!(parse_state("unavailable"), InterfaceState::Unavailable);
        assert_eq!(parse_state("unmanaged"), InterfaceState::Down);
    }

    #[test]
    fn test_parse_type_variants() {
        assert_eq!(parse_type("wifi"), InterfaceType::Wifi);
        assert_eq!(parse_type("ethernet"), InterfaceType::Ethernet);
        assert_eq!(parse_type("wifi-p2p"), InterfaceType::Unknown);
        assert_eq!(parse_type("loopback"), InterfaceType::Unknown);
    }
}
