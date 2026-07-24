use nimbus_core::error::Result;
use tokio::process::Command;

pub async fn scan_available_networks(interface: &str) -> Result<Vec<ScannedNetwork>> {
    let _ = Command::new("iw")
        .args(["dev", interface, "scan"])
        .output()
        .await;

    let output = Command::new("iw")
        .args(["dev", interface, "scan", "dump"])
        .output()
        .await?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_iw_scan_dump(&stdout)
}

#[derive(Debug, Clone)]
pub struct ScannedNetwork {
    pub ssid: String,
    pub bssid: String,
    pub frequency: u32,
    pub signal_dbm: i32,
    pub channel: u32,
}

fn parse_iw_scan_dump(output: &str) -> Result<Vec<ScannedNetwork>> {
    let mut networks = Vec::new();
    let mut current_bssid = String::new();
    let mut current_freq = 0u32;
    let mut current_signal = 0i32;
    let mut current_ssid = String::new();

    for line in output.lines() {
        if let Some(bssid) = line.strip_prefix("\tBSS ") {
            if !current_bssid.is_empty() && !current_ssid.is_empty() {
                networks.push(ScannedNetwork {
                    ssid: current_ssid.clone(),
                    bssid: current_bssid.clone(),
                    frequency: current_freq,
                    signal_dbm: current_signal,
                    channel: freq_to_channel(current_freq),
                });
            }
            current_bssid = bssid.split_whitespace().next().unwrap_or("").to_string();
            current_ssid.clear();
            current_freq = 0;
            current_signal = 0;
        } else if let Some(freq) = line.strip_prefix("\tfreq: ") {
            current_freq = freq.trim().parse().unwrap_or(0);
        } else if let Some(signal) = line.strip_prefix("\tsignal: ") {
            current_signal = signal
                .trim()
                .strip_suffix(" dBm")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
        } else if let Some(ssid) = line.strip_prefix("\tSSID: ") {
            current_ssid = ssid.trim().to_string();
        }
    }

    if !current_bssid.is_empty() && !current_ssid.is_empty() {
        networks.push(ScannedNetwork {
            ssid: current_ssid,
            bssid: current_bssid,
            frequency: current_freq,
            signal_dbm: current_signal,
            channel: freq_to_channel(current_freq),
        });
    }

    Ok(networks)
}

pub fn freq_to_channel(freq: u32) -> u32 {
    match freq {
        2412 => 1,
        2417 => 2,
        2422 => 3,
        2427 => 4,
        2432 => 5,
        2437 => 6,
        2442 => 7,
        2447 => 8,
        2452 => 9,
        2457 => 10,
        2462 => 11,
        2467 => 12,
        2472 => 13,
        f if (5170..=5825).contains(&f) => (f - 5000) / 5,
        f if (5955..=7115).contains(&f) => (f - 5950) / 5,
        _ => 0,
    }
}
