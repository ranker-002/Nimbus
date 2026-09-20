use serde::{Deserialize, Serialize};

use nimbus_core::error::{NimbusError, Result};
use nimbus_core::types::{Band, HotspotConfig, Security};

const CONFIG_DIR: &str = ".config/nimbus-hotspot";
const PREFS_FILE: &str = "preferences.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppPreferences {
    pub auto_start: bool,
    pub default_band: Band,
    pub default_security: Security,
    /// Devices allowed on a new hotspot, or 0 for no limit.
    pub default_max_clients: u32,
    /// Regulatory domain to apply to new hotspots, or `None` to leave the
    /// machine's own setting untouched.
    #[serde(default)]
    pub default_country: Option<String>,
    #[serde(default)]
    pub dark_mode_only: bool,
    pub show_notifications: bool,
    /// The hotspot last started through Nimbus, so auto-start and the form can
    /// be seeded with what the user actually used.
    #[serde(default)]
    pub last_config: Option<HotspotConfig>,
}

impl Default for AppPreferences {
    fn default() -> Self {
        Self {
            auto_start: false,
            default_band: Band::Auto,
            default_security: Security::Wpa2Wpa3Transition,
            // No limit by default: a hotspot should not turn devices away
            // unless the user asked it to.
            default_max_clients: 0,
            default_country: None,
            dark_mode_only: false,
            show_notifications: true,
            last_config: None,
        }
    }
}

fn config_dir() -> Result<std::path::PathBuf> {
    let home = std::env::var("HOME")
        .map_err(|_| NimbusError::ConfigError("HOME environment variable not set".into()))?;
    let dir = std::path::PathBuf::from(home).join(CONFIG_DIR);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn preferences_path() -> Result<std::path::PathBuf> {
    Ok(config_dir()?.join(PREFS_FILE))
}

/// Whether the user already ran Nimbus once (a preferences file exists).
pub fn preferences_exist() -> bool {
    preferences_path().map(|p| p.exists()).unwrap_or(false)
}

pub fn load_preferences() -> AppPreferences {
    let path = match preferences_path() {
        Ok(p) => p,
        Err(_) => return AppPreferences::default(),
    };
    let Ok(data) = std::fs::read_to_string(&path) else {
        return AppPreferences::default();
    };
    match serde_json::from_str(&data) {
        Ok(prefs) => prefs,
        Err(e) => {
            // Never overwrite what the user had, but do not silently ignore a
            // broken file either.
            log::error!(
                "Could not read {} ({}); falling back to defaults",
                path.display(),
                e
            );
            AppPreferences::default()
        }
    }
}

pub fn save_preferences(prefs: &AppPreferences) -> Result<()> {
    let path = preferences_path()?;
    let data = serde_json::to_string_pretty(prefs)?;

    // Write beside the real file and rename, so an interrupted save can never
    // leave a truncated preferences file behind.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}
