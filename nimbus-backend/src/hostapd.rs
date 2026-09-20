//! Runs the access point directly with hostapd.
//!
//! Used for the backend that keeps the machine online: hostapd drives a
//! dedicated AP interface (see [`nimbus_network::virtual_ap`]) while the
//! original device keeps its station connection.
//!
//! Going through hostapd instead of NetworkManager also unlocks two things NM
//! cannot express: a real WPA2/WPA3 transition AP (`wpa_key_mgmt=WPA-PSK SAE`)
//! and a hard cap on associated stations (`max_num_sta`).

use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};

use nimbus_core::error::{NimbusError, Result};
use nimbus_core::types::{Band, HotspotConfig, Security};

/// Builds a hostapd configuration for `config` on `interface`, fixed to
/// `channel`.
///
/// The channel is passed in rather than taken from the config because the AP
/// has to share the station connection's channel on single-channel radios.
pub fn build_config(config: &HotspotConfig, interface: &str, channel: u32) -> String {
    let mut lines = vec![
        format!("interface={}", interface),
        "driver=nl80211".to_string(),
        // The SSID goes in hex so that no byte in it can be read as
        // configuration. `ssid=` would let a newline inject directives.
        format!("ssid2={}", hex_ssid(&config.ssid)),
        "utf8_ssid=1".to_string(),
        format!("channel={}", channel),
        format!("hw_mode={}", hw_mode(channel)),
        "wmm_enabled=1".to_string(),
        // Open System authentication; WPA is negotiated separately below.
        "auth_algs=1".to_string(),
        format!("ignore_broadcast_ssid={}", u8::from(config.hidden)),
    ];

    if channel >= 36 {
        lines.push("ieee80211n=1".to_string());
        lines.push("ieee80211ac=1".to_string());
    } else {
        lines.push("ieee80211n=1".to_string());
    }

    if let Some(country) = &config.country_code {
        lines.push(format!("country_code={}", country));
        // Advertise the regulatory domain to clients.
        lines.push("ieee80211d=1".to_string());
    }

    if nimbus_core::types::is_dfs_channel(channel) {
        // Radar detection: hostapd listens before transmitting, which takes at
        // least a minute. Without this the driver refuses the channel outright.
        lines.push("ieee80211h=1".to_string());
    }

    if let Some(max) = config.max_clients {
        lines.push(format!("max_num_sta={}", max));
    }

    if config.client_isolation {
        lines.push("ap_isolate=1".to_string());
    }

    lines.extend(security_lines(config));
    lines.push(String::new());
    lines.join("\n")
}

/// The `wpa*` block for the requested security mode.
fn security_lines(config: &HotspotConfig) -> Vec<String> {
    match config.security {
        Security::Open => Vec::new(),
        Security::Wpa2 => vec![
            "wpa=2".into(),
            "wpa_key_mgmt=WPA-PSK".into(),
            "rsn_pairwise=CCMP".into(),
            format!("wpa_passphrase={}", config.password),
        ],
        Security::Wpa3 => vec![
            "wpa=2".into(),
            "wpa_key_mgmt=SAE".into(),
            "rsn_pairwise=CCMP".into(),
            // WPA3 mandates protected management frames.
            "ieee80211w=2".into(),
            format!("sae_password={}", config.password),
        ],
        // A genuine transition AP: WPA2 and WPA3 clients both associate, with
        // management frame protection optional so WPA2 devices still work.
        Security::Wpa2Wpa3Transition => vec![
            "wpa=2".into(),
            "wpa_key_mgmt=WPA-PSK SAE".into(),
            "rsn_pairwise=CCMP".into(),
            "ieee80211w=1".into(),
            format!("wpa_passphrase={}", config.password),
            format!("sae_password={}", config.password),
        ],
    }
}

/// Runtime directory for the hostapd configuration. On `/run`, which is a
/// tmpfs, so the passphrase never touches a disk.
const RUNTIME_DIR: &str = "/run/nimbus";

/// Writes the configuration where hostapd can read it, readable only by root.
///
/// The mode is set at creation rather than afterwards: a `create` then `chmod`
/// would leave a window in which the passphrase was world-readable.
async fn write_config(text: &str) -> Result<std::path::PathBuf> {
    // tokio's OpenOptions carries the unix `mode` setter itself.
    use std::os::unix::fs::PermissionsExt;

    tokio::fs::create_dir_all(RUNTIME_DIR).await.map_err(|e| {
        NimbusError::ConfigError(format!("Could not create {}: {}", RUNTIME_DIR, e))
    })?;
    tokio::fs::set_permissions(RUNTIME_DIR, std::fs::Permissions::from_mode(0o700)).await?;

    let path = std::path::Path::new(RUNTIME_DIR).join("hostapd.conf");
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)
        .await
        .map_err(|e| {
            NimbusError::ConfigError(format!("Could not write the hostapd configuration: {}", e))
        })?;

    file.write_all(text.as_bytes()).await?;
    file.flush().await?;
    Ok(path)
}

/// Streams hostapd's output into `sink` and signals when the AP is on air.
///
/// Reading continuously matters for more than diagnostics: hostapd blocks once
/// its pipe buffer fills, so an unread pipe would eventually stall it.
fn spawn_reader(
    child: &mut Child,
    sink: Arc<Mutex<Vec<String>>>,
) -> tokio::sync::watch::Receiver<bool> {
    let (tx, rx) = tokio::sync::watch::channel(false);

    let mut streams: Vec<Box<dyn tokio::io::AsyncRead + Send + Unpin>> = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        streams.push(Box::new(stdout));
    }
    if let Some(stderr) = child.stderr.take() {
        streams.push(Box::new(stderr));
    }

    for stream in streams {
        let sink = Arc::clone(&sink);
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stream).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                log::debug!("hostapd: {}", line);

                // hostapd announces readiness once beaconing starts; with DFS
                // this only comes after radar detection completes.
                if line.contains("AP-ENABLED") || line.contains("->ENABLED") {
                    let _ = tx.send(true);
                }

                if let Ok(mut sink) = sink.lock() {
                    sink.push(line);
                    let overflow = sink.len().saturating_sub(LOG_TAIL);
                    if overflow > 0 {
                        sink.drain(..overflow);
                    }
                }
            }
        });
    }

    rx
}

/// Condenses hostapd's output into something that fits in a notification.
///
/// hostapd prints a banner and per-line driver chatter before the line that
/// actually explains the refusal, so the tail carries the useful part.
fn summarise(output: &str) -> String {
    let lines: Vec<&str> = output
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();

    if lines.is_empty() {
        return "no output".to_string();
    }
    lines
        .iter()
        .rev()
        .take(3)
        .rev()
        .copied()
        .collect::<Vec<_>>()
        .join(" | ")
}

fn hw_mode(channel: u32) -> &'static str {
    match Band::of_channel(channel) {
        Band::Band2_4Ghz => "g",
        _ => "a",
    }
}

/// Hex-encodes the SSID for hostapd's `ssid2` directive.
fn hex_ssid(ssid: &str) -> String {
    ssid.as_bytes()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect()
}

/// Whether hostapd is installed.
pub async fn is_available() -> bool {
    which("hostapd").await
}

pub(crate) async fn which(program: &str) -> bool {
    Command::new(program)
        .arg("-v")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .is_ok()
}

/// How long to wait for the access point to come on air. Radar detection makes
/// this slow, so DFS channels get a much longer allowance.
const READY_TIMEOUT: Duration = Duration::from_secs(20);
const READY_TIMEOUT_DFS: Duration = Duration::from_secs(90);
/// Lines of hostapd output kept for diagnosing a failure.
const LOG_TAIL: usize = 40;

/// A running hostapd process.
pub struct Hostapd {
    child: Child,
    /// The tail of hostapd's output, filled by a background reader.
    output: Arc<Mutex<Vec<String>>>,
}

impl Hostapd {
    /// Starts hostapd on the given configuration.
    ///
    /// hostapd only reads its configuration from a file — it takes `-` as a
    /// literal filename, not as standard input. The file therefore holds the
    /// passphrase, so it is created under `/run` (tmpfs, so it never reaches
    /// persistent storage), readable only by root, and deleted as soon as
    /// hostapd has read it.
    /// Waits until hostapd reports the access point is on air, rather than
    /// assuming success because the process is still alive.
    ///
    /// An earlier version slept 1.5 s and treated "not yet exited" as running.
    /// hostapd sets the interface up over several seconds — longer still with
    /// radar detection — and dies partway through if the channel is not usable.
    /// The result was a zombie hostapd and an application reporting a hotspot
    /// that was never on air.
    pub async fn start(config_text: &str, dfs: bool) -> Result<Self> {
        let config_path = write_config(config_text).await?;

        let mut child = Command::new("hostapd")
            .arg(&config_path)
            // hostapd reports both progress and refusals on *stdout*, not
            // stderr. Discarding it leaves nothing but an exit code.
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                NimbusError::ConfigError(format!(
                    "Could not start hostapd: {}. Install it to run a hotspot \
                     that keeps your Wi-Fi connection.",
                    e
                ))
            })?;

        let output = Arc::new(Mutex::new(Vec::new()));
        let enabled = spawn_reader(&mut child, Arc::clone(&output));

        let timeout = if dfs {
            READY_TIMEOUT_DFS
        } else {
            READY_TIMEOUT
        };
        let mut hostapd = Self { child, output };
        let outcome = hostapd.await_ready(enabled, timeout).await;

        // hostapd reads the file once at start-up; it need not linger.
        let _ = tokio::fs::remove_file(&config_path).await;

        outcome?;
        Ok(hostapd)
    }

    /// Resolves once the AP is on air, or with an error explaining why not.
    async fn await_ready(
        &mut self,
        mut enabled: tokio::sync::watch::Receiver<bool>,
        timeout: Duration,
    ) -> Result<()> {
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            if *enabled.borrow_and_update() {
                return Ok(());
            }

            // hostapd gone means the channel or configuration was refused.
            if let Some(status) = self.child.try_wait()? {
                let tail = self.recent_output();
                log::error!("hostapd exited ({}):\n{}", status, tail);
                return Err(NimbusError::ConfigError(format!(
                    "hostapd could not bring the access point up ({}): {}",
                    status,
                    summarise(&tail)
                )));
            }

            if tokio::time::Instant::now() >= deadline {
                let tail = self.recent_output();
                log::error!("hostapd timed out:\n{}", tail);
                return Err(NimbusError::ConfigError(format!(
                    "The access point did not come on air within {}s: {}",
                    timeout.as_secs(),
                    summarise(&tail)
                )));
            }

            let _ = tokio::time::timeout(Duration::from_millis(250), enabled.changed()).await;
        }
    }

    /// The tail of hostapd's output collected so far.
    fn recent_output(&self) -> String {
        self.output
            .lock()
            .map(|lines| lines.join("\n"))
            .unwrap_or_default()
    }

    /// Stops hostapd and waits for it to exit.
    pub async fn stop(mut self) {
        if let Err(e) = self.child.kill().await {
            log::warn!("Could not stop hostapd: {}", e);
        }
        let _ = self.child.wait().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> HotspotConfig {
        HotspotConfig {
            ssid: "MonHotspot".into(),
            password: "motdepasse123".into(),
            ..Default::default()
        }
    }

    #[test]
    fn ssid_is_hex_encoded() {
        // "Hi" -> 48 69
        assert_eq!(hex_ssid("Hi"), "4869");
        // Non-ASCII survives as its UTF-8 bytes.
        assert_eq!(hex_ssid("é"), "c3a9");
    }

    /// A newline in the SSID must not be able to add a hostapd directive.
    #[test]
    fn ssid_cannot_inject_configuration() {
        let mut cfg = config();
        cfg.ssid = "evil\nap_isolate=0".into();
        let text = build_config(&cfg, "nimbus-ap", 36);

        assert!(!text.contains("evil"));
        assert_eq!(text.lines().filter(|l| l.starts_with("ssid2=")).count(), 1);
        assert!(!text.lines().any(|l| l == "ap_isolate=0"));
    }

    #[test]
    fn hw_mode_follows_the_channel() {
        assert_eq!(hw_mode(6), "g");
        assert_eq!(hw_mode(11), "g");
        assert_eq!(hw_mode(36), "a");
        assert_eq!(hw_mode(112), "a");
    }

    #[test]
    fn config_pins_the_requested_channel() {
        let text = build_config(&config(), "nimbus-ap", 100);
        assert!(text.contains("channel=100"));
        assert!(text.contains("hw_mode=a"));
        assert!(text.contains("interface=nimbus-ap"));
    }

    #[test]
    fn wpa2_uses_a_passphrase() {
        let mut cfg = config();
        cfg.security = Security::Wpa2;
        let text = build_config(&cfg, "nimbus-ap", 36);
        assert!(text.contains("wpa_key_mgmt=WPA-PSK"));
        assert!(text.contains("wpa_passphrase=motdepasse123"));
        assert!(!text.contains("SAE"));
    }

    #[test]
    fn wpa3_requires_protected_management_frames() {
        let mut cfg = config();
        cfg.security = Security::Wpa3;
        let text = build_config(&cfg, "nimbus-ap", 36);
        assert!(text.contains("wpa_key_mgmt=SAE"));
        assert!(text.contains("sae_password=motdepasse123"));
        assert!(text.contains("ieee80211w=2"));
    }

    /// The mode NetworkManager cannot express at all.
    #[test]
    fn transition_mode_offers_both_wpa2_and_wpa3() {
        let mut cfg = config();
        cfg.security = Security::Wpa2Wpa3Transition;
        let text = build_config(&cfg, "nimbus-ap", 36);
        assert!(text.contains("wpa_key_mgmt=WPA-PSK SAE"));
        assert!(text.contains("wpa_passphrase=motdepasse123"));
        assert!(text.contains("sae_password=motdepasse123"));
        // Optional, so WPA2-only devices can still associate.
        assert!(text.contains("ieee80211w=1"));
    }

    #[test]
    fn open_network_has_no_wpa_block() {
        let mut cfg = config();
        cfg.security = Security::Open;
        cfg.password = String::new();
        let text = build_config(&cfg, "nimbus-ap", 36);
        assert!(!text.contains("wpa"));
    }

    #[test]
    fn max_clients_becomes_max_num_sta() {
        let mut cfg = config();
        cfg.max_clients = Some(5);
        assert!(build_config(&cfg, "nimbus-ap", 36).contains("max_num_sta=5"));
    }

    #[test]
    fn no_limit_leaves_max_num_sta_unset() {
        let mut cfg = config();
        cfg.max_clients = None;
        assert!(!build_config(&cfg, "nimbus-ap", 36).contains("max_num_sta"));
    }

    #[test]
    fn country_code_is_advertised() {
        let mut cfg = config();
        cfg.country_code = Some("FR".into());
        let text = build_config(&cfg, "nimbus-ap", 36);
        assert!(text.contains("country_code=FR"));
        assert!(text.contains("ieee80211d=1"));
    }

    #[test]
    fn hidden_and_isolation_flags() {
        let mut cfg = config();
        cfg.hidden = true;
        cfg.client_isolation = true;
        let text = build_config(&cfg, "nimbus-ap", 36);
        assert!(text.contains("ignore_broadcast_ssid=1"));
        assert!(text.contains("ap_isolate=1"));
    }

    /// Channel 100 is DFS, which is exactly where this machine's Wi-Fi sits.
    #[test]
    fn dfs_channel_enables_radar_detection() {
        let text = build_config(&config(), "nimbus-ap", 100);
        assert!(text.contains("ieee80211h=1"));
    }

    #[test]
    fn non_dfs_channel_skips_radar_detection() {
        for channel in [6, 36, 149] {
            let text = build_config(&config(), "nimbus-ap", channel);
            assert!(
                !text.contains("ieee80211h"),
                "channel {} should not need DFS",
                channel
            );
        }
    }

    #[test]
    fn visible_network_sets_broadcast_flag_to_zero() {
        let text = build_config(&config(), "nimbus-ap", 36);
        assert!(text.contains("ignore_broadcast_ssid=0"));
        assert!(!text.contains("ap_isolate"));
    }
}
