use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use nimbus_core::error::{NimbusError, Result};
use nimbus_core::types::HotspotConfig;

const CONFIG_DIR: &str = ".config/nimbus-hotspot";
const HOTSPOTS_FILE: &str = "hotspots.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedHotspot {
    pub uuid: String,
    pub name: String,
    pub config: HotspotConfig,
    pub created_at: String,
    pub last_used: Option<String>,
    pub use_count: u32,
}

fn config_path() -> Result<PathBuf> {
    let home = std::env::var("HOME")
        .map_err(|_| NimbusError::ConfigError("HOME not set".into()))?;
    let dir = PathBuf::from(home).join(CONFIG_DIR);
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join(HOTSPOTS_FILE))
}

pub fn load_hotspots() -> Result<Vec<SavedHotspot>> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let data = std::fs::read_to_string(&path)?;
    let hotspots: Vec<SavedHotspot> = serde_json::from_str(&data)?;
    Ok(hotspots)
}

pub fn save_hotspots(hotspots: &[SavedHotspot]) -> Result<()> {
    let path = config_path()?;
    let data = serde_json::to_string_pretty(hotspots)?;
    std::fs::write(&path, data)?;
    Ok(())
}

pub fn add_hotspot(config: &HotspotConfig) -> Result<SavedHotspot> {
    let mut hotspots = load_hotspots()?;
    let uuid = uuid::Uuid::new_v4().to_string();

    let saved = SavedHotspot {
        uuid: uuid.clone(),
        name: config.ssid.clone(),
        config: config.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
        last_used: None,
        use_count: 0,
    };

    hotspots.push(saved.clone());
    save_hotspots(&hotspots)?;
    Ok(saved)
}

pub fn delete_hotspot(uuid: &str) -> Result<()> {
    let mut hotspots = load_hotspots()?;
    hotspots.retain(|h| h.uuid != uuid);
    save_hotspots(&hotspots)?;
    Ok(())
}

pub fn update_hotspot(uuid: &str, config: &HotspotConfig) -> Result<()> {
    let mut hotspots = load_hotspots()?;
    if let Some(h) = hotspots.iter_mut().find(|h| h.uuid == uuid) {
        h.config = config.clone();
        h.name = config.ssid.clone();
        save_hotspots(&hotspots)?;
        Ok(())
    } else {
        Err(NimbusError::ConfigError(format!(
            "Hotspot '{}' not found",
            uuid
        )))
    }
}

pub fn mark_used(uuid: &str) -> Result<()> {
    let mut hotspots = load_hotspots()?;
    if let Some(h) = hotspots.iter_mut().find(|h| h.uuid == uuid) {
        h.last_used = Some(chrono::Utc::now().to_rfc3339());
        h.use_count += 1;
        save_hotspots(&hotspots)?;
    }
    Ok(())
}
