use tokio::process::Command;

use nimbus_core::error::{NimbusError, Result};

#[derive(Debug, Clone, Default)]
pub struct IwPhyInfo {
    pub supports_ap: bool,
    pub supports_wpa3: bool,
    pub supports_wifi_6: bool,
    pub supports_wifi_6e: bool,
    pub supports_wifi_7: bool,
    /// The radio advertises a combination that holds a managed (STA) interface
    /// and an AP interface at once.
    pub can_do_sta_and_ap: bool,
    /// That combination caps `#channels` at 1, so the AP has to share the
    /// station connection's channel to coexist with it.
    pub sta_ap_same_channel_only: bool,
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

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// Returns the body that follows the line containing `header`, stopping at the
/// first line indented no more deeply than the header itself.
///
/// `iw phy info` nests everything under tab-indented headings, so a section
/// ends where the indentation drops back — not at the first unindented line.
fn extract_section(output: &str, header: &str) -> String {
    let mut lines = output.lines();
    let header_indent = loop {
        match lines.next() {
            Some(line) if line.contains(header) => break indent_of(line),
            Some(_) => continue,
            None => return String::new(),
        }
    };

    let mut body = String::new();
    for line in lines {
        if !line.trim().is_empty() && indent_of(line) <= header_indent {
            break;
        }
        body.push_str(line);
        body.push('\n');
    }
    body
}

/// One `* #{ managed, P2P-client } <= 2, #{ AP } <= 1, total <= 3, #channels <= 1`
/// entry from the "valid interface combinations" section.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Combination {
    /// Each `#{ ... } <= N` group: the interface modes it covers and its limit.
    groups: Vec<(Vec<String>, u32)>,
    total: Option<u32>,
    max_channels: Option<u32>,
}

impl Combination {
    /// Whether this entry lets a station connection and an AP exist together.
    fn allows_sta_and_ap(&self) -> bool {
        if self.total.is_some_and(|t| t < 2) {
            return false;
        }

        let has = |group: &(Vec<String>, u32), mode: &str| {
            group.0.iter().any(|m| m.eq_ignore_ascii_case(mode))
        };

        // Either two separate groups, one holding managed and one holding AP...
        for (i, sta_group) in self.groups.iter().enumerate() {
            if !has(sta_group, "managed") || sta_group.1 < 1 {
                continue;
            }
            for (j, ap_group) in self.groups.iter().enumerate() {
                if !has(ap_group, "AP") {
                    continue;
                }
                if i != j && ap_group.1 >= 1 {
                    return true;
                }
                // ...or a single group covering both, with room for two of them.
                if i == j && ap_group.1 >= 2 {
                    return true;
                }
            }
        }
        false
    }
}

/// Splits the combinations section into entries. Each entry starts with `*` and
/// may wrap over several indented continuation lines.
fn parse_combinations(section: &str) -> Vec<Combination> {
    let mut entries: Vec<String> = Vec::new();

    for line in section.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed
            .strip_prefix("* ")
            .or_else(|| trimmed.strip_prefix('*'))
        {
            entries.push(rest.trim().to_string());
        } else if !trimmed.is_empty() && !trimmed.ends_with(':') {
            if let Some(last) = entries.last_mut() {
                last.push(' ');
                last.push_str(trimmed);
            }
        }
    }

    entries.iter().map(|e| parse_combination(e)).collect()
}

fn parse_combination(entry: &str) -> Combination {
    let mut groups = Vec::new();
    let mut rest = entry;

    // Walk the `#{ a, b } <= N` groups in order.
    while let Some(open) = rest.find("#{") {
        let after_open = &rest[open + 2..];
        let Some(close) = after_open.find('}') else {
            break;
        };
        let modes: Vec<String> = after_open[..close]
            .split(',')
            .map(|m| m.trim().to_string())
            .filter(|m| !m.is_empty())
            .collect();

        let tail = &after_open[close + 1..];
        let limit = parse_limit(tail).unwrap_or(1);
        groups.push((modes, limit));
        rest = tail;
    }

    Combination {
        groups,
        total: find_limit_after(entry, "total"),
        max_channels: find_limit_after(entry, "#channels"),
    }
}

/// Reads the `<= N` that immediately follows, e.g. from ` <= 2, #{ AP } <= 1`.
fn parse_limit(tail: &str) -> Option<u32> {
    let tail = tail.trim_start();
    let rest = tail.strip_prefix("<=")?;
    rest.trim_start()
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse().ok())
}

/// Reads `<keyword> <= N` anywhere in the entry.
fn find_limit_after(entry: &str, keyword: &str) -> Option<u32> {
    let pos = entry.find(keyword)?;
    parse_limit(&entry[pos + keyword.len()..])
}

/// Reads the first integer following `keyword`, whichever punctuation separates
/// them (`#max{ 8 }` and `#max <= 8` both yield 8).
fn find_number_after(text: &str, keyword: &str) -> Option<u32> {
    let pos = text.find(keyword)?;
    let tail = &text[pos + keyword.len()..];
    let start = tail.find(|c: char| c.is_ascii_digit())?;
    tail[start..]
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .and_then(|s| s.parse().ok())
}

fn parse_iw_output(output: &str) -> IwPhyInfo {
    let mut info = IwPhyInfo::default();

    let mode_text = extract_section(output, "Supported interface modes:");
    info.supports_ap = mode_text
        .lines()
        .any(|l| l.trim().trim_start_matches("* ").trim() == "AP");

    let combo_text = extract_section(output, "valid interface combinations:");
    let combinations = parse_combinations(&combo_text);
    // When several entries allow STA+AP, keep the least restrictive one.
    let sta_ap = combinations
        .iter()
        .filter(|c| c.allows_sta_and_ap())
        .max_by_key(|c| c.max_channels.unwrap_or(u32::MAX));
    info.can_do_sta_and_ap = sta_ap.is_some();
    info.sta_ap_same_channel_only = sta_ap.is_some_and(|c| c.max_channels == Some(1));

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
            36, 40, 44, 48, 52, 56, 60, 64, 100, 104, 108, 112, 116, 120, 124, 128, 132, 136, 140,
            149, 153, 157, 161, 165,
        ];
    }

    info.max_sta = find_number_after(&combo_text, "#max").unwrap_or(32);

    info
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The combinations block a single-radio Intel adapter reports: it can hold
    /// a station connection and an AP together, but only on one channel.
    const SINGLE_RADIO_COMBOS: &str = "\tvalid interface combinations:\n\
         \t\t * #{ managed, P2P-client } <= 2, #{ P2P-GO } <= 1, #{ P2P-device } <= 1,\n\
         \t\t   total <= 3, #channels <= 2\n\
         \t\t * #{ managed, P2P-client } <= 2, #{ AP } <= 1, #{ P2P-device } <= 1,\n\
         \t\t   total <= 3, #channels <= 1\n\
         \tHT Capability overrides:\n";

    #[test]
    fn test_extract_section_found() {
        let output = "Header1\n  line1\n  line2\nNextHeader\n";
        let section = extract_section(output, "Header1");
        assert_eq!(section, "  line1\n  line2\n");
    }

    #[test]
    fn test_extract_section_not_found() {
        let output = "Some other content";
        let section = extract_section(output, "Header1");
        assert_eq!(section, "");
    }

    #[test]
    fn test_extract_section_to_end() {
        let output = "Header1\n  line1\n  line2\n";
        let section = extract_section(output, "Header1");
        assert_eq!(section, "  line1\n  line2\n");
    }

    #[test]
    fn test_extract_section_stops_at_sibling_heading() {
        // Both headings are tab-indented, as in real `iw phy info` output.
        let output = "\tSection A:\n\t\tvalue a\n\tSection B:\n\t\tvalue b\n";
        assert_eq!(extract_section(output, "Section A:"), "\t\tvalue a\n");
        assert_eq!(extract_section(output, "Section B:"), "\t\tvalue b\n");
    }

    #[test]
    fn test_parse_iw_output_ap_support() {
        let output = "Supported interface modes:\n  * AP\n  * managed\n";
        let info = parse_iw_output(output);
        assert!(info.supports_ap);
    }

    #[test]
    fn test_parse_iw_output_no_ap_support() {
        let output = "Supported interface modes:\n  * managed\n";
        let info = parse_iw_output(output);
        assert!(!info.supports_ap);
    }

    #[test]
    fn test_ap_vlan_alone_is_not_ap_support() {
        // "AP/VLAN" must not be mistaken for AP mode.
        let output = "Supported interface modes:\n  * managed\n  * AP/VLAN\n";
        let info = parse_iw_output(output);
        assert!(!info.supports_ap);
    }

    #[test]
    fn test_parse_iw_output_wpa3_support() {
        let output = "Supported extended features:\n  SAE\n";
        let info = parse_iw_output(output);
        assert!(info.supports_wpa3);
    }

    #[test]
    fn test_parse_iw_output_wifi6_support() {
        let output = "[HE] some feature\n";
        let info = parse_iw_output(output);
        assert!(info.supports_wifi_6);
        assert!(!info.supports_wifi_7);
    }

    #[test]
    fn test_parse_iw_output_wifi6e_support() {
        let output = "6 GHz support\n";
        let info = parse_iw_output(output);
        assert!(info.supports_wifi_6e);
    }

    #[test]
    fn test_parse_iw_output_wifi7_support() {
        let output = "EHT support\n";
        let info = parse_iw_output(output);
        assert!(info.supports_wifi_7);
        assert!(info.supports_wifi_6); // WiFi 7 implies WiFi 6
    }

    #[test]
    fn test_parse_iw_output_no_false_positive_the() {
        let output = "THE quick brown fox\n";
        let info = parse_iw_output(output);
        assert!(!info.supports_wifi_6);
    }

    #[test]
    fn test_parse_iw_output_channels() {
        let output = "Frequencies:\n  * 2412 MHz (1)\n  * 2437 MHz (6)\n  * 5180 MHz (36)\n";
        let info = parse_iw_output(output);
        assert!(info.channels_2ghz.contains(&1));
        assert!(info.channels_2ghz.contains(&6));
        assert!(info.channels_5ghz.contains(&36));
    }

    #[test]
    fn test_parse_iw_output_sta_ap_combo() {
        let output = "valid interface combinations:\n  * #{ managed } <= 1, #{ AP } <= 1, #{ } <= 1, total <= 2, #channels <= 1\n";
        let info = parse_iw_output(output);
        assert!(info.can_do_sta_and_ap);
    }

    #[test]
    fn test_parse_iw_output_max_sta() {
        let output =
            "valid interface combinations:\n  * #{ managed } <= 1, #{ AP } <= 1, #max{ 8 }\n";
        let info = parse_iw_output(output);
        assert_eq!(info.max_sta, 8);
    }

    #[test]
    fn test_parse_iw_output_default_channels() {
        let output = "";
        let info = parse_iw_output(output);
        assert_eq!(info.channels_2ghz.len(), 13);
        assert!(!info.channels_5ghz.is_empty());
    }

    #[test]
    fn test_wrapped_combination_entries_are_joined() {
        let combos = parse_combinations(SINGLE_RADIO_COMBOS);
        assert_eq!(combos.len(), 2);
        assert_eq!(combos[0].total, Some(3));
        assert_eq!(combos[0].max_channels, Some(2));
        assert_eq!(combos[1].max_channels, Some(1));
    }

    #[test]
    fn test_single_radio_supports_sta_ap_on_one_channel() {
        let info = parse_iw_output(SINGLE_RADIO_COMBOS);
        assert!(info.can_do_sta_and_ap);
        // This is the flag that keeps the app from knocking the user offline:
        // the AP has to land on the station connection's channel.
        assert!(info.sta_ap_same_channel_only);
    }

    #[test]
    fn test_group_limits_are_parsed_per_group() {
        let combos = parse_combinations(SINGLE_RADIO_COMBOS);
        let second = &combos[1];
        assert_eq!(second.groups.len(), 3);
        assert_eq!(second.groups[0].0, ["managed", "P2P-client"]);
        assert_eq!(second.groups[0].1, 2);
        assert_eq!(second.groups[1].0, ["AP"]);
        assert_eq!(second.groups[1].1, 1);
    }

    #[test]
    fn test_dual_channel_radio_is_not_flagged_same_channel_only() {
        let output = "valid interface combinations:\n\
             \t\t * #{ managed } <= 1, #{ AP } <= 1, total <= 2, #channels <= 2\n";
        let info = parse_iw_output(output);
        assert!(info.can_do_sta_and_ap);
        assert!(!info.sta_ap_same_channel_only);
    }

    #[test]
    fn test_shared_group_without_headroom_rejects_sta_ap() {
        // "#{ managed, AP } <= 1" means one *or* the other, never both.
        let output =
            "valid interface combinations:\n  * #{ managed, AP } <= 1, total <= 1, #channels <= 1\n";
        let info = parse_iw_output(output);
        assert!(!info.can_do_sta_and_ap);
    }

    #[test]
    fn test_shared_group_with_headroom_allows_sta_ap() {
        let output =
            "valid interface combinations:\n  * #{ managed, AP } <= 2, total <= 2, #channels <= 1\n";
        let info = parse_iw_output(output);
        assert!(info.can_do_sta_and_ap);
    }

    #[test]
    fn test_ap_only_radio_cannot_do_sta_ap() {
        let output =
            "valid interface combinations:\n  * #{ AP } <= 1, total <= 1, #channels <= 1\n";
        let info = parse_iw_output(output);
        assert!(!info.can_do_sta_and_ap);
    }

    #[test]
    fn test_no_combinations_section_reports_no_concurrency() {
        let info = parse_iw_output("Supported interface modes:\n  * AP\n");
        assert!(!info.can_do_sta_and_ap);
        assert!(!info.sta_ap_same_channel_only);
    }
}
