use std::collections::HashMap;

use chrono::Utc;
use mac_address::MacAddress;
use tokio::process::Command;

use nimbus_core::error::{NimbusError, Result};
use nimbus_core::types::StationInfo;

pub async fn get_stations(interface: &str) -> Result<Vec<StationInfo>> {
    let output = Command::new("iw")
        .args(["dev", interface, "station", "dump"])
        .output()
        .await
        .map_err(|e| NimbusError::IwError(format!("Failed to run iw: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(NimbusError::IwError(format!(
            "iw station dump failed: {}",
            stderr
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_iw_station_dump(&stdout)
}

/// Disassociates a station from the AP running on `interface`.
///
/// Sends a deauthentication (subtype 0xC) rather than a plain disassociation,
/// so the client treats the link as gone and does not silently keep using it.
/// Requires `CAP_NET_ADMIN`.
pub async fn disconnect_station(interface: &str, mac: &MacAddress) -> Result<()> {
    let mac = mac.to_string();
    let output = Command::new("iw")
        .args(["dev", interface, "station", "del", &mac, "subtype", "0xC"])
        .output()
        .await
        .map_err(|e| NimbusError::IwError(format!("Failed to run iw: {}", e)))?;

    if !output.status.success() {
        return Err(NimbusError::IwError(format!(
            "Could not disconnect {}: {}",
            mac,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
}

fn parse_iw_station_dump(output: &str) -> Result<Vec<StationInfo>> {
    let mut stations = Vec::new();
    let mut current_mac: Option<MacAddress> = None;
    let mut props: HashMap<String, String> = HashMap::new();

    for line in output.lines() {
        if let Some(rest) = line.strip_prefix("Station ") {
            if let Some(mac_str) = rest.split_whitespace().next() {
                if let Some(mac) = current_mac {
                    if let Some(station) = build_station(mac, &props) {
                        stations.push(station);
                    }
                }
                current_mac = MacAddress::from_str(mac_str);
                props.clear();
            }
        } else if let Some((key, value)) = line.trim().split_once(':') {
            props.insert(key.trim().to_string(), value.trim().to_string());
        }
    }

    if let Some(mac) = current_mac {
        if let Some(station) = build_station(mac, &props) {
            stations.push(station);
        }
    }

    Ok(stations)
}

fn build_station(mac: MacAddress, props: &HashMap<String, String>) -> Option<StationInfo> {
    let signal_dbm = props
        .get("signal avg")
        .or_else(|| props.get("signal"))
        .and_then(|s| s.trim().strip_suffix(" dBm"))
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(-100);

    let signal_percent = (((signal_dbm as f64 + 100.0) * 2.0).round() as i32).clamp(0, 100) as u8;

    let rx_bytes = props
        .get("rx bytes")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    let tx_bytes = props
        .get("tx bytes")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    let rx_rate = props
        .get("rx bitrate")
        .and_then(|s| s.split_whitespace().next())
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0);
    let tx_rate = props
        .get("tx bitrate")
        .and_then(|s| s.split_whitespace().next())
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0);

    let connected_secs = props
        .get("connected time")
        .and_then(|s| s.trim().strip_suffix(" sec"))
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);

    Some(StationInfo {
        mac,
        ip: None,
        hostname: None,
        manufacturer: None,
        signal_dbm,
        signal_percent,
        rx_bytes,
        tx_bytes,
        rx_rate_mbps: rx_rate,
        tx_rate_mbps: tx_rate,
        connected_since: Utc::now() - chrono::Duration::seconds(connected_secs),
    })
}

pub trait MacAddressExt {
    fn from_str(s: &str) -> Option<MacAddress>;
}

impl MacAddressExt for MacAddress {
    fn from_str(s: &str) -> Option<MacAddress> {
        let cleaned: String = s
            .chars()
            .filter(|c| c.is_ascii_hexdigit() || *c == ':')
            .collect();
        let bytes: Vec<u8> = cleaned
            .split(':')
            .filter_map(|h| u8::from_str_radix(h, 16).ok())
            .collect();
        if bytes.len() == 6 {
            let mut arr = [0u8; 6];
            arr.copy_from_slice(&bytes);
            Some(MacAddress::new(arr))
        } else {
            None
        }
    }
}
