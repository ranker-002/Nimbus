use nimbus_core::error::Result;
use nimbus_core::types::ScannedNetwork;
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

fn parse_iw_scan_dump(output: &str) -> Result<Vec<ScannedNetwork>> {
    let mut networks = Vec::new();
    let mut current_bssid = String::new();
    let mut current_freq = 0u32;
    let mut current_signal = 0i32;
    let mut current_ssid = String::new();

    for line in output.lines() {
        let trimmed = line.trim_start();
        if let Some(bssid) = trimmed.strip_prefix("BSS ") {
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
        } else if let Some(freq) = trimmed.strip_prefix("freq: ") {
            current_freq = freq.trim().parse().unwrap_or(0);
        } else if let Some(signal) = trimmed.strip_prefix("signal: ") {
            current_signal = signal
                .trim()
                .strip_suffix(" dBm")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
        } else if let Some(ssid) = trimmed.strip_prefix("SSID: ") {
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

pub use nimbus_core::types::freq_to_channel;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_freq_to_channel_2_4ghz() {
        assert_eq!(freq_to_channel(2412), 1);
        assert_eq!(freq_to_channel(2437), 6);
        assert_eq!(freq_to_channel(2462), 11);
        assert_eq!(freq_to_channel(2472), 13);
    }

    #[test]
    fn test_freq_to_channel_5ghz() {
        assert_eq!(freq_to_channel(5180), 36);
        assert_eq!(freq_to_channel(5240), 48);
        assert_eq!(freq_to_channel(5745), 149);
        assert_eq!(freq_to_channel(5825), 165);
    }

    #[test]
    fn test_freq_to_channel_6ghz() {
        assert_eq!(freq_to_channel(5955), 1);
        assert_eq!(freq_to_channel(6035), 17);
        assert_eq!(freq_to_channel(6115), 33);
    }

    #[test]
    fn test_freq_to_channel_unknown() {
        assert_eq!(freq_to_channel(0), 0);
        assert_eq!(freq_to_channel(1000), 0);
        assert_eq!(freq_to_channel(9999), 0);
    }

    #[test]
    fn test_parse_iw_scan_dump() {
        let output = "BSS 00:11:22:33:44:55 (on wlan0)\n\tfreq: 2437\n\tsignal: -50.00 dBm\n\tSSID: TestNetwork\nBSS aa:bb:cc:dd:ee:ff (on wlan0)\n\tfreq: 5180\n\tsignal: -60.00 dBm\n\tSSID: AnotherNetwork\n";
        let networks = parse_iw_scan_dump(output).unwrap();
        assert_eq!(networks.len(), 2);
        assert_eq!(networks[0].ssid, "TestNetwork");
        assert_eq!(networks[0].bssid, "00:11:22:33:44:55");
        assert_eq!(networks[0].frequency, 2437);
        assert_eq!(networks[0].channel, 6);
        assert_eq!(networks[1].ssid, "AnotherNetwork");
    }

    #[test]
    fn test_parse_iw_scan_dump_empty() {
        let output = "";
        let networks = parse_iw_scan_dump(output).unwrap();
        assert!(networks.is_empty());
    }

    #[test]
    fn test_parse_iw_scan_dump_hidden_ssid() {
        let output = "BSS 00:11:22:33:44:55 (on wlan0)\n\tfreq: 2437\n\tsignal: -50.00 dBm\n";
        let networks = parse_iw_scan_dump(output).unwrap();
        assert!(networks.is_empty());
    }
}
