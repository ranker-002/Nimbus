use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NimbusSettings {
    pub saved_hotspots: Vec<SavedHotspotEntry>,
    pub preferences: crate::preferences::AppPreferences,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedHotspotEntry {
    pub uuid: String,
    pub name: String,
    pub config: nimbus_core::types::HotspotConfig,
}
