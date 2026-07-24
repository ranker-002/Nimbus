use serde::{Deserialize, Serialize};

use nimbus_core::error::{NimbusError, Result};
use nimbus_core::types::{Band, Security};

const CONFIG_DIR: &str = ".config/nimbus-hotspot";
const PREFS_FILE: &str = "preferences.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppPreferences {
    pub auto_start: bool,
    pub default_band: Band,
    pub default_security: Security,
    pub default_max_clients: u32,
    pub password_rotation: bool,
    pub password_rotation_hours: u32,
    pub dark_mode_only: bool,
    pub show_notifications: bool,
}

impl Default for AppPreferences {
    fn default() -> Self {
        Self {
            auto_start: false,
            default_band: Band::Auto,
            default_security: Security::Wpa2Wpa3Transition,
            default_max_clients: 10,
            password_rotation: false,
            password_rotation_hours: 24,
            dark_mode_only: true,
            show_notifications: true,
        }
    }
}

fn prefs_path() -> Result<std::path::PathBuf> {
    let home = std::env::var("HOME")
        .map_err(|_| NimbusError::ConfigError("HOME environment variable not set".into()))?;
    let dir = std::path::PathBuf::from(home).join(CONFIG_DIR);
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join(PREFS_FILE))
}

pub fn load_preferences() -> AppPreferences {
    let path = match prefs_path() {
        Ok(p) => p,
        Err(_) => return AppPreferences::default(),
    };
    if !path.exists() {
        return AppPreferences::default();
    }
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok())
        .unwrap_or_default()
}

pub fn save_preferences(prefs: &AppPreferences) -> Result<()> {
    let path = prefs_path()?;
    let data = serde_json::to_string_pretty(prefs)?;
    std::fs::write(&path, data)?;
    Ok(())
}
