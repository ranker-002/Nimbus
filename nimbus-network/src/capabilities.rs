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
        let after_header = &output[start + header.len()..];
        // Find the end of the section: next line that starts with a non-whitespace char
        for (i, line) in after_header.lines().enumerate() {
            if i == 0 {
                continue; // skip the rest of the header line
            }
            if !line.starts_with(char::is_whitespace) && !line.is_empty() {
                // Calculate the byte offset back to the start of this section
                let section_end = output[start + header.len()..]
                    .lines()
                    .take(i)
                    .map(|l| l.len() + 1) // +1 for newline
                    .sum::<usize>();
                return &output[start..start + header.len() + section_end];
            }
        }
        // Section extends to end of output
        after_header
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
        && (combo_text.contains("#channels <= 1") || combo_text.contains("#{{ 1 }}"));

    let feat_text = extract_section(output, "Supported extended features:");
    info.supports_wpa3 = feat_text.contains("SAE") || feat_text.contains("SAE_OFFLOAD");

    // WiFi 7 detection (EHT = Extremely High Throughput, 802.11be)
    if output.contains("EHT") || output.contains("802.11be") {
        info.supports_wifi_7 = true;
        info.supports_wifi_6 = true;
    }
    // WiFi 6 detection (HE = High Efficiency, 802.11ax)
    // Use bracketed form to avoid false positives from English words like "THE"
    else if output.contains("[HE]") || output.contains("802.11ax") {
        info.supports_wifi_6 = true;
    }

    if output.contains("6 GHz") || output.contains("6GHz") {
        info.supports_wifi_6e = true;
    }

    // Parse frequencies from the "Frequencies:" section
    let freq_section = extract_section(output, "Frequencies:");
    for line in freq_section.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix('*') {
            let rest = rest.trim_start();
            if let Some(freq_str) = rest.strip_suffix("MHz") {
                let freq: u32 = freq_str.trim().parse().unwrap_or(0);
                if freq == 0 {
                    continue;
                }

                // Parse channel from the parenthesized number after "MHz"
                let channel = if let Some(paren_start) = line.find('(') {
                    if let Some(paren_end) = line[paren_start..].find(')') {
                        let chan_str = &line[paren_start + 1..paren_start + paren_end];
                        chan_str.parse::<u32>().unwrap_or(0)
                    } else {
                        0
                    }
                } else {
                    0
                };

                if channel == 0 {
                    continue;
                }

                if (2400..=2500).contains(&freq) {
                    info.channels_2ghz.push(channel);
                } else if (5000..=5900).contains(&freq) || (5955..=7115).contains(&freq) {
                    info.channels_5ghz.push(channel);
                }
            }
        }
    }

    if info.channels_2ghz.is_empty() {
        info.channels_2ghz = (1..=13).collect();
    }
    if info.channels_5ghz.is_empty() {
        info.channels_5ghz = vec![
            36, 40, 44, 48, 52, 56, 60, 64, 100, 104, 108, 112, 116, 120, 124, 128, 132, 136,
            140, 149, 153, 157, 161, 165,
        ];
    }

    // Parse max_sta from "valid interface combinations" section
    if let Some(max_pos) = combo_text.find("#max") {
        let rest = &combo_text[max_pos..];
        if let Some(brace_start) = rest.find('{') {
            if let Some(brace_end) = rest[brace_start..].find('}') {
                let max_str = &rest[brace_start + 1..brace_start + brace_end];
                if let Ok(max) = max_str.trim().parse::<u32>() {
                    info.max_sta = max;
                }
            }
        }
    }
    if info.max_sta == 0 {
        info.max_sta = 32;
    }

    info
}
