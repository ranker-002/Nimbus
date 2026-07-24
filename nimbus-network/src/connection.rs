use std::collections::HashMap;

use nimbus_core::types::HotspotConfig;

pub struct ConnectionManager;

impl ConnectionManager {
    pub fn build_connection_settings(
        config: &HotspotConfig,
    ) -> HashMap<String, HashMap<String, zbus::zvariant::Value<'static>>> {
        let mut settings = HashMap::new();

        let mut conn = HashMap::new();
        conn.insert(
            "type".to_string(),
            zbus::zvariant::Value::Str(zbus::zvariant::Str::from("802-11-wireless")),
        );
        conn.insert(
            "id".to_string(),
            zbus::zvariant::Value::Str(zbus::zvariant::Str::from(
                format!("Nimbus-{}", config.ssid),
            )),
        );
        settings.insert("connection".to_string(), conn);

        let mut wifi = HashMap::new();
        let ssid_bytes: Vec<u8> = config.ssid.as_bytes().to_vec();
        wifi.insert(
            "ssid".to_string(),
            zbus::zvariant::Value::Array(zbus::zvariant::Array::from(ssid_bytes)),
        );
        wifi.insert(
            "mode".to_string(),
            zbus::zvariant::Value::Str(zbus::zvariant::Str::from("ap")),
        );
        settings.insert("802-11-wireless".to_string(), wifi);

        let mut wsec = HashMap::new();
        match config.security {
            nimbus_core::types::Security::Open => {
                wsec.insert(
                    "key-mgmt".to_string(),
                    zbus::zvariant::Value::Str(zbus::zvariant::Str::from("none")),
                );
            }
            nimbus_core::types::Security::Wpa2 => {
                wsec.insert(
                    "key-mgmt".to_string(),
                    zbus::zvariant::Value::Str(zbus::zvariant::Str::from("wpa-psk")),
                );
                wsec.insert(
                    "psk".to_string(),
                    zbus::zvariant::Value::Str(zbus::zvariant::Str::from(
                        config.password.clone(),
                    )),
                );
            }
            nimbus_core::types::Security::Wpa3 => {
                wsec.insert(
                    "key-mgmt".to_string(),
                    zbus::zvariant::Value::Str(zbus::zvariant::Str::from("sae")),
                );
                wsec.insert(
                    "psk".to_string(),
                    zbus::zvariant::Value::Str(zbus::zvariant::Str::from(
                        config.password.clone(),
                    )),
                );
            }
            nimbus_core::types::Security::Wpa2Wpa3Transition => {
                wsec.insert(
                    "key-mgmt".to_string(),
                    zbus::zvariant::Value::Str(zbus::zvariant::Str::from("wpa-psk sae")),
                );
                wsec.insert(
                    "psk".to_string(),
                    zbus::zvariant::Value::Str(zbus::zvariant::Str::from(
                        config.password.clone(),
                    )),
                );
            }
        }
        settings.insert("802-11-wireless-security".to_string(), wsec);

        let mut ip4 = HashMap::new();
        ip4.insert(
            "method".to_string(),
            zbus::zvariant::Value::Str(zbus::zvariant::Str::from("shared")),
        );
        settings.insert("ipv4".to_string(), ip4);

        let mut ip6 = HashMap::new();
        ip6.insert(
            "method".to_string(),
            zbus::zvariant::Value::Str(zbus::zvariant::Str::from("ignore")),
        );
        settings.insert("ipv6".to_string(), ip6);

        settings
    }
}
