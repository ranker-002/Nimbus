use nimbus_core::error::Result;
use nimbus_core::types::{InterfaceState, InterfaceType, NetworkInterface};

use mac_address::MacAddress;
use tokio::process::Command;

use crate::manager::parse_mac;

pub async fn detect_interfaces() -> Result<Vec<NetworkInterface>> {
    let mut result = Vec::new();

    // Use a non-colon separator to avoid MAC address splitting issues
    let output = Command::new("nmcli")
        .args([
            "-t", "-f", "DEVICE,TYPE,STATE,HWADDR", "device", "status",
            "separator", "|",
        ])
        .output()
        .await.map_err(|_| std::io::Error::other(
                "separator not supported",
            ));

    match output {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            for line in stdout.lines() {
                let parts: Vec<&str> = line.split('|').collect();
                if parts.len() < 3 {
                    continue;
                }
                let name = parts[0].to_string();
                let iface_type = match parts[1] {
                    "wifi" => InterfaceType::Wifi,
                    "ethernet" => InterfaceType::Ethernet,
                    "usb" => InterfaceType::UsbTethering,
                    "bridge" => InterfaceType::Bridge,
                    _ => InterfaceType::Unknown,
                };
                let state = match parts[2] {
                    "connected" => InterfaceState::Up,
                    "disconnected" => InterfaceState::Disconnected,
                    "unavailable" => InterfaceState::Unavailable,
                    "unmanaged" => InterfaceState::Down,
                    "connecting" | "disconnecting" => InterfaceState::Disconnected,
                    _ => InterfaceState::Disconnected,
                };

                let mac_str = parts.get(3).copied().unwrap_or("00:00:00:00:00:00");
                let mac = parse_mac(mac_str);

                result.push(NetworkInterface {
                    name,
                    interface_type: iface_type,
                    mac,
                    state,
                    driver: String::new(),
                });
            }
        }
        _ => {
            // Fallback: parse the standard colon-separated nmcli output
            // nmcli -t -f DEVICE,TYPE,STATE device status
            let output = Command::new("nmcli")
                .args(["-t", "-f", "DEVICE,TYPE,STATE", "device", "status"])
                .output()
                .await?;

            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    // Parse carefully: DEVICE:TYPE:STATE
                    // MAC addresses are NOT in this output, so simple split is fine
                    let parts: Vec<&str> = line.split(':').collect();
                    if parts.len() < 3 {
                        continue;
                    }
                    let name = parts[0].to_string();
                    let iface_type = match parts[1] {
                        "wifi" => InterfaceType::Wifi,
                        "ethernet" => InterfaceType::Ethernet,
                        "usb" => InterfaceType::UsbTethering,
                        "bridge" => InterfaceType::Bridge,
                        _ => InterfaceType::Unknown,
                    };
                    let state = match parts[2] {
                        "connected" => InterfaceState::Up,
                        "disconnected" => InterfaceState::Disconnected,
                        "unavailable" => InterfaceState::Unavailable,
                        "unmanaged" => InterfaceState::Down,
                        _ => InterfaceState::Disconnected,
                    };

                    result.push(NetworkInterface {
                        name,
                        interface_type: iface_type,
                        mac: MacAddress::new([0; 6]),
                        state,
                        driver: String::new(),
                    });
                }
            }
        }
    }

    Ok(result)
}

pub async fn get_upstream_interface() -> Result<Option<String>> {
    let output = Command::new("nmcli")
        .args(["-t", "-f", "DEVICE,TYPE,STATE", "device", "status"])
        .output()
        .await?;

    if !output.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut best: Option<(String, u8)> = None;

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() < 3 || parts[2] != "connected" {
            continue;
        }
        let priority = match parts[1] {
            "ethernet" => 3,
            "wifi" => 2,
            "usb" => 1,
            _ => 0,
        };
        if best.as_ref().is_none_or(|(_, bp)| priority > *bp) {
            best = Some((parts[0].to_string(), priority));
        }
    }

    Ok(best.map(|(name, _)| name))
}
