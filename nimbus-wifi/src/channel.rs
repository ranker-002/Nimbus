use nimbus_core::error::Result;
use nimbus_core::types::Band;
use tokio::process::Command;

use crate::capabilities::IwPhyInfo;
use crate::scanner::freq_to_channel;

pub async fn select_best_channel(
    interface: &str,
    band: &Band,
    info: &IwPhyInfo,
) -> Result<u32> {
    match band {
        Band::Band2_4Ghz => {
            let used = scan_used_channels(interface).await?;
            let available: Vec<u32> = (1..=13)
                .filter(|ch| !used.contains(ch))
                .collect();
            Ok(pick_least_congested(&available, &[1, 6, 11]))
        }
        Band::Band5Ghz => {
            let used = scan_used_channels(interface).await?;
            let available: Vec<u32> = info
                .channels_5ghz
                .iter()
                .copied()
                .filter(|ch| !used.contains(ch))
                .collect();
            let preferred = vec![36, 40, 44, 48, 149, 153, 157, 161];
            Ok(pick_least_congested(&available, &preferred))
        }
        Band::Auto => {
            if !info.channels_5ghz.is_empty() {
                let used = scan_used_channels(interface).await?;
                let available: Vec<u32> = info
                    .channels_5ghz
                    .iter()
                    .copied()
                    .filter(|ch| !used.contains(ch))
                    .collect();
                let preferred = vec![36, 40, 44, 48, 149, 153, 157, 161];
                Ok(pick_least_congested(&available, &preferred))
            } else {
                let used = scan_used_channels(interface).await?;
                let available: Vec<u32> = (1..=13)
                    .filter(|ch| !used.contains(ch))
                    .collect();
                Ok(pick_least_congested(&available, &[1, 6, 11]))
            }
        }
    }
}

async fn scan_used_channels(interface: &str) -> Result<Vec<u32>> {
    let _ = Command::new("iw")
        .args(["dev", interface, "scan"])
        .output()
        .await;

    let output = Command::new("iw")
        .args(["dev", interface, "scan", "dump"])
        .output()
        .await?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut channels = Vec::new();

    for line in stdout.lines() {
        if let Some(freq_part) = line.strip_prefix("\tfreq: ") {
            if let Ok(freq) = freq_part.trim().parse::<u32>() {
                let channel = freq_to_channel(freq);
                if channel > 0 {
                    channels.push(channel);
                }
            }
        }
    }

    Ok(channels)
}

fn pick_least_congested(available: &[u32], preferred: &[u32]) -> u32 {
    for ch in preferred {
        if available.contains(ch) {
            return *ch;
        }
    }
    available.first().copied().unwrap_or(6)
}
