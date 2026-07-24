use thiserror::Error;

pub type Result<T> = std::result::Result<T, NimbusError>;

#[derive(Debug, Error)]
pub enum NimbusError {
    #[error("NetworkManager not available: {0}")]
    NetworkManagerUnavailable(String),

    #[error("Interface '{0}' not found or not a Wi-Fi adapter")]
    InterfaceNotFound(String),

    #[error("Adapter '{0}' does not support Access Point mode")]
    ApModeNotSupported(String),

    #[error("Failed to create hotspot: {0}")]
    HotspotCreationFailed(String),

    #[error("WPA3 not supported on adapter '{0}'")]
    Wpa3NotSupported(String),

    #[error("5GHz band not supported on adapter '{0}'")]
    Band5GhzNotSupported(String),

    #[error("Password too short (minimum 8 characters for WPA2/WPA3)")]
    PasswordTooShort,

    #[error("nftables error: {0}")]
    NftablesError(String),

    #[error("iw command failed: {0}")]
    IwError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("Database error: {0}")]
    DatabaseError(#[from] rusqlite::Error),

    #[error("D-Bus error: {0}")]
    DbusError(#[from] zbus::Error),

    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("No upstream interface found")]
    NoUpstreamInterface,

    #[error("Hotspot not active")]
    HotspotNotActive,

    #[error("Invalid value: {0}")]
    InvalidValue(String),
}
