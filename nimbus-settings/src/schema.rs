use serde::{Deserialize, Serialize};

use crate::preferences::AppPreferences;
use crate::hotspots::SavedHotspot;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NimbusSettings {
    pub saved_hotspots: Vec<SavedHotspot>,
    pub preferences: AppPreferences,
}

impl NimbusSettings {
    pub fn load() -> Self {
        let preferences = crate::preferences::load_preferences();
        let saved_hotspots = crate::hotspots::load_hotspots().unwrap_or_default();
        Self {
            saved_hotspots,
            preferences,
        }
    }

    pub fn save(&self) -> nimbus_core::error::Result<()> {
        crate::preferences::save_preferences(&self.preferences)?;
        crate::hotspots::save_hotspots(&self.saved_hotspots)?;
        Ok(())
    }
}
