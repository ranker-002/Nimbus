//! The wireless regulatory domain.
//!
//! The regulatory domain decides which channels and transmit powers are legal,
//! and the kernel keeps exactly one of it for the whole machine — there is no
//! per-connection setting. So changing it for a hotspot changes it for every
//! Wi-Fi interface, including any connection the machine is currently using.
//!
//! Because of that, Nimbus treats a domain change as an explicit, reversible
//! action: [`HotspotConfig::country_code`] is `None` by default, meaning "leave
//! the system domain alone", and [`RegDomainGuard`] puts back whatever was
//! there before.
//!
//! [`HotspotConfig::country_code`]: nimbus_core::types::HotspotConfig::country_code

use tokio::process::Command;

use nimbus_core::error::{NimbusError, Result};

/// The ISO 3166-1 alpha-2 code the kernel uses for "no specific country".
pub const WORLD: &str = "00";

/// Whether `code` is something `iw reg set` will accept: two ASCII letters, or
/// the world domain `00`.
pub fn is_valid(code: &str) -> bool {
    code == WORLD || (code.len() == 2 && code.chars().all(|c| c.is_ascii_alphabetic()))
}

/// Normalises a user-supplied country code to the uppercase form `iw` expects.
pub fn normalize(code: &str) -> String {
    code.trim().to_ascii_uppercase()
}

/// Reads the kernel's current global regulatory domain.
///
/// Returns `None` when `iw` is unavailable or reports nothing usable, which
/// callers should treat as "unknown" rather than as an error — a hotspot can
/// still be started without knowing the domain.
pub async fn current() -> Option<String> {
    let output = Command::new("iw")
        .args(["reg", "get"])
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_reg_get(&String::from_utf8_lossy(&output.stdout))
}

/// Extracts the global country code from `iw reg get` output.
///
/// The output starts with a `global` block followed by per-phy blocks; only the
/// global one is the machine-wide setting, so parsing stops at the first
/// `country` line.
fn parse_reg_get(output: &str) -> Option<String> {
    for line in output.lines() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("country ") else {
            continue;
        };
        // e.g. "country 00: DFS-UNSET" or "country FR: DFS-ETSI"
        let code = rest.split(':').next()?.trim();
        if is_valid(code) {
            return Some(code.to_ascii_uppercase());
        }
    }
    None
}

/// The kernel's regulatory database, shipped by the `wireless-regdb` package.
///
/// Without it the kernel cannot resolve a country to a set of rules and stays
/// on the world domain, where 5 GHz is receive-only — so no access point can
/// transmit there.
pub const REGULATORY_DB: &str = "/usr/lib/firmware/regulatory.db";

/// Whether the regulatory database is installed.
pub async fn database_present() -> bool {
    tokio::fs::metadata(REGULATORY_DB).await.is_ok()
}

/// Tells the kernel to switch to `code`.
///
/// Requires `CAP_NET_ADMIN`. Note that the kernel may decline or clamp the
/// request — with some cards the domain is baked into the hardware — so callers
/// should not assume [`current`] will match afterwards.
pub async fn set(code: &str) -> Result<()> {
    let code = normalize(code);
    if !is_valid(&code) {
        return Err(NimbusError::InvalidValue(format!(
            "'{}' is not a two-letter country code",
            code
        )));
    }

    let output = Command::new("iw")
        .args(["reg", "set", &code])
        .output()
        .await
        .map_err(|e| NimbusError::IwError(format!("Failed to run iw reg set: {}", e)))?;

    if !output.status.success() {
        return Err(NimbusError::IwError(format!(
            "Could not set regulatory domain to {}: {}",
            code,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
}

/// Why the kernel might have ignored a domain change, phrased as a next step.
///
/// The database is loaded once, early in boot. Installing `wireless-regdb`
/// afterwards leaves the file on disk while the kernel is still running without
/// it — a state where every check looks fine but no country can be applied.
async fn refusal_hint() -> &'static str {
    if !database_present().await {
        return "the regulatory database is missing — install the wireless-regdb package.";
    }
    "the database is installed but was not loaded at boot, so the kernel is \
     still without it. Reboot, or reload the Wi-Fi driver, to pick it up."
}

/// Applies a regulatory domain and remembers the previous one so it can be put
/// back when the hotspot stops.
#[derive(Debug, Default)]
pub struct RegDomainGuard {
    previous: Option<String>,
}

impl RegDomainGuard {
    pub fn new() -> Self {
        Self::default()
    }

    /// Switches to `code` unless the machine is already using it. Does nothing
    /// and reports `Ok(false)` when no change was needed.
    pub async fn apply(&mut self, code: &str) -> Result<bool> {
        let code = normalize(code);
        let existing = current().await;

        if existing.as_deref() == Some(code.as_str()) {
            return Ok(false);
        }

        set(&code).await?;

        // `iw reg set` succeeds even when the kernel ignores it, so confirm.
        // Leaving the machine on the world domain quietly forbids 5 GHz
        // transmission, which shows up much later as an unexplained hostapd
        // failure.
        if current().await.as_deref() != Some(code.as_str()) {
            return Err(NimbusError::IwError(format!(
                "The regulatory domain is still {}, not {}: {}",
                current().await.as_deref().unwrap_or("unknown"),
                code,
                refusal_hint().await
            )));
        }

        // Only record the first change: a second apply without a restore must
        // not overwrite the domain the machine started with.
        if self.previous.is_none() {
            self.previous = Some(existing.unwrap_or_else(|| WORLD.to_string()));
        }
        Ok(true)
    }

    /// Puts back the domain recorded by [`apply`](Self::apply). A no-op when
    /// nothing was changed.
    pub async fn restore(&mut self) -> Result<()> {
        let Some(previous) = self.previous.take() else {
            return Ok(());
        };
        set(&previous).await
    }

    /// The domain in force before Nimbus changed it, if it did.
    pub fn previous(&self) -> Option<&str> {
        self.previous.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real output from a machine with no country configured.
    const WORLD_OUTPUT: &str = "global\ncountry 00: DFS-UNSET\n\t(2402 - 2472 @ 40), (6, 20), (N/A)\n\t(5170 - 5250 @ 80), (6, 20), (N/A), AUTO-BW, PASSIVE-SCAN\n";

    const FRANCE_OUTPUT: &str =
        "global\ncountry FR: DFS-ETSI\n\t(2402 - 2482 @ 40), (N/A, 20), (N/A)\n";

    #[test]
    fn parses_the_world_domain() {
        assert_eq!(parse_reg_get(WORLD_OUTPUT).as_deref(), Some("00"));
    }

    #[test]
    fn parses_a_country_domain() {
        assert_eq!(parse_reg_get(FRANCE_OUTPUT).as_deref(), Some("FR"));
    }

    #[test]
    fn takes_the_global_block_not_a_later_phy_block() {
        let output = "global\ncountry FR: DFS-ETSI\n\nphy#0\ncountry US: DFS-FCC\n";
        assert_eq!(parse_reg_get(output).as_deref(), Some("FR"));
    }

    #[test]
    fn uppercases_a_lowercase_country() {
        assert_eq!(
            parse_reg_get("country de: DFS-ETSI\n").as_deref(),
            Some("DE")
        );
    }

    #[test]
    fn returns_none_without_a_country_line() {
        assert_eq!(parse_reg_get("global\n\t(2402 - 2472 @ 40)\n"), None);
        assert_eq!(parse_reg_get(""), None);
    }

    #[test]
    fn rejects_a_malformed_country_line() {
        assert_eq!(parse_reg_get("country XYZ: DFS-UNSET\n"), None);
    }

    #[test]
    fn validates_country_codes() {
        assert!(is_valid("FR"));
        assert!(is_valid("us"));
        assert!(is_valid("00"));
        assert!(!is_valid("USA"));
        assert!(!is_valid("F"));
        assert!(!is_valid(""));
        assert!(!is_valid("1A"));
    }

    #[test]
    fn normalizes_to_uppercase() {
        assert_eq!(normalize(" fr "), "FR");
        assert_eq!(normalize("Us"), "US");
    }

    #[tokio::test]
    async fn set_rejects_an_invalid_code_without_running_iw() {
        assert!(set("NOPE").await.is_err());
    }

    /// Whichever state the machine is in, the hint has to name an action —
    /// "the kernel refused it" leaves the user with nowhere to go.
    #[tokio::test]
    async fn the_refusal_hint_always_names_a_next_step() {
        let hint = refusal_hint().await;
        assert!(
            hint.contains("wireless-regdb") || hint.contains("Reboot"),
            "unhelpful hint: {}",
            hint
        );
    }

    #[tokio::test]
    async fn restore_without_a_change_does_nothing() {
        let mut guard = RegDomainGuard::new();
        assert!(guard.previous().is_none());
        assert!(guard.restore().await.is_ok());
    }
}
