pub const APP_ID: &str = "com.nimbus.Hotspot";
pub const APP_NAME: &str = "Nimbus Hotspot";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

pub const DEFAULT_SSID: &str = "Nimbus-Hotspot";
pub const DEFAULT_CHANNEL: u32 = 6;
pub const DEFAULT_COUNTRY: &str = "US";
pub const MIN_PASSWORD_LEN: usize = 8;
pub const MAX_PASSWORD_LEN: usize = 63;
pub const MAX_SSID_LEN: usize = 32;
pub const MAX_CLIENTS_DEFAULT: u32 = 10;
pub const MAX_CLIENTS_MAX: u32 = 64;

pub const BANDWIDTH_SAMPLE_INTERVAL_MS: u64 = 2000;
pub const STATION_REFRESH_INTERVAL_MS: u64 = 2000;

pub const NM_BUS_NAME: &str = "org.freedesktop.NetworkManager";
pub const NM_SETTINGS_PATH: &str = "/org/freedesktop/NetworkManager/Settings";
pub const NM_PATH: &str = "/org/freedesktop/NetworkManager";

pub const SETTINGS_SCHEMA: &str = "com.nimbus.Hotspot";
pub const SETTINGS_PATH: &str = "/com/nimbus/Hotspot/";

pub const HISTORY_DB_PATH: &str = ".local/share/nimbus-hotspot/history.db";
