use tokio::process::Command;

use nimbus_core::error::{NimbusError, Result};

#[derive(Debug, Clone, Default)]
pub struct IwPhyInfo {
    pub supports_ap: bool,
    pub supports_wpa3: bool,
    pub supports_wifi_6: bool,
    pub supports_wifi_6e: bool,
    pub supports_wifi_7: bool,
    pub can_do_sta_and_ap: bool,
    pub channels_2ghz: Vec<u32>,
    pub channels_5ghz: Vec<u32>,
    pub max_sta: u32,
}

pub async fn parse_iw_phy_info(interface: &str) -> Result<IwPhyInfo> {
    let phy_name = get_phy_for_interface(interface).await?;
    let output = Command::new("iw")
        .args(["phy", &phy_name, "info"])
        .output()
        .await
        .map_err(|e| NimbusError::IwError(format!("Failed to run iw: {}", e)))?;

    if !output.status.success() {
        return Err(NimbusError::IwError("iw phy info failed".into()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_iw_output(&stdout))
}

async fn get_phy_for_interface(interface: &str) -> Result<String> {
    let output = Command::new("iw")
        .args(["dev", interface, "info"])
        .output()
        .await
        .map_err(|e| NimbusError::IwError(format!("Failed to run iw: {}", e)))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if let Some(wphy) = line
            .strip_prefix("\twiphy ")
            .or_else(|| line.strip_prefix("wiphy "))
        {
            return Ok(format!("phy{}", wphy.trim()));
        }
    }
    Err(NimbusError::IwError(format!(
        "Could not determine PHY for {}",
        interface
    )))
}

fn extract_section<'a>(output: &'a str, header: &str) -> &'a str {
    if let Some(start) = output.find(header) {
        let rest = &output[start..];
        for line in rest[header.len()..].lines() {
            if !line.starts_with(|c: char| c.is_whitespace()) && !line.is_empty() {
                return &rest[..rest.len() - line.len() - 1];
            }
        }
        rest
    } else {
        ""
    }
}

fn parse_iw_output(output: &str) -> IwPhyInfo {
    let mut info = IwPhyInfo::default();

    let mode_text = extract_section(output, "Supported interface modes:");
    info.supports_ap = mode_text.contains("* AP");

    let combo_text = extract_section(output, "valid interface combinations:");
    info.can_do_sta_and_ap = combo_text.contains("managed")
        && combo_text.contains("AP")
        && combo_text.contains("#channels <= 1");

    let feat_text = extract_section(output, "Supported extended features:");
    info.supports_wpa3 = feat_text.contains("SAE") || feat_text.contains("SAE_OFFLOAD");

    if output.contains("EHT") || output.contains("802.11be") {
        info.supports_wifi_7 = true;
        info.supports_wifi_6 = true;
    } else if output.contains("HE") || output.contains("802.11ax") {
        info.supports_wifi_6 = true;
    }

    if output.contains("6 GHz") || output.contains("6GHz") {
        info.supports_wifi_6e = true;
    }

    for line in output.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix('*') {
            let rest = rest.trim_start();
            if let Some(freq_str) = rest.strip_suffix("MHz") {
                let freq: u32 = freq_str.trim().parse().unwrap_or(0);
                if (2400..=2500).contains(&freq) {
                    if let Some(chan) = output.lines().find(|l| l.contains(&format!("{} MHz", freq))) {
                        if let Some(paren) = chan.find('(') {
                            if let Some(paren_end) = chan[paren..].find(')') {
                                let chan_str = &chan[paren + 1..paren + paren_end];
                                if let Ok(channel) = chan_str.parse::<u32>() {
                                    info.channels_2ghz.push(channel);
                                }
                            }
                        }
                    }
                } else if (5000..=5900).contains(&freq) {
                    if let Some(chan) = output.lines().find(|l| l.contains(&format!("{} MHz", freq))) {
                        if let Some(paren) = chan.find('(') {
                            if let Some(paren_end) = chan[paren..].find(')') {
                                let chan_str = &chan[paren + 1..paren + paren_end];
                                if let Ok(channel) = chan_str.parse::<u32>() {
                                    info.channels_5ghz.push(channel);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if info.channels_2ghz.is_empty() {
        info.channels_2ghz = (1..=14).collect();
    }
    if info.channels_5ghz.is_empty() {
        info.channels_5ghz = vec![
            36, 40, 44, 48, 52, 56, 60, 64, 100, 104, 108, 112, 116, 120, 124, 128, 132, 136,
            140, 149, 153, 157, 161, 165,
        ];
    }

    info.max_sta = 32;

    info
}
