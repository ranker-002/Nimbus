use zbus::connection::Builder;
use zbus::interface;

use nimbus_core::types::{Band, HotspotConfig, Security};
use nimbus_network::manager::NmManager;
use nimbus_network::traits::NetworkManagerApi;

struct NimbusDbus {
    nm: NmManager,
}

#[interface(name = "com.nimbus.Hotspot")]
impl NimbusDbus {
    async fn start_hotspot(
        &self,
        ssid: &str,
        password: &str,
        band: &str,
    ) -> zbus::fdo::Result<String> {
        let config = HotspotConfig {
            ssid: ssid.to_string(),
            password: password.to_string(),
            band: match band {
                "2.4" => Band::Band2_4Ghz,
                "5" => Band::Band5Ghz,
                _ => Band::Auto,
            },
            security: Security::Wpa2Wpa3Transition,
            ..Default::default()
        };

        if let Err(e) = config.validate() {
            return Err(zbus::fdo::Error::Failed(format!(
                "Invalid configuration: {}",
                e
            )));
        }

        let wifi_devices = self.nm.get_wifi_devices().await.map_err(|e| {
            zbus::fdo::Error::Failed(e.to_string())
        })?;

        let interface = wifi_devices.first().map(|d| d.name.clone()).ok_or_else(|| {
            zbus::fdo::Error::Failed("No WiFi adapter found".into())
        })?;

        match self.nm.create_hotspot(&config, &interface).await {
            Ok(info) => Ok(format!("Started on {}", info.interface)),
            Err(e) => Err(zbus::fdo::Error::Failed(e.to_string())),
        }
    }

    async fn stop_hotspot(&self) -> zbus::fdo::Result<String> {
        match self.nm.stop_hotspot().await {
            Ok(()) => Ok("Stopped".into()),
            Err(e) => Err(zbus::fdo::Error::Failed(e.to_string())),
        }
    }

    async fn get_status(&self) -> zbus::fdo::Result<String> {
        match self.nm.get_active_hotspot().await {
            Ok(Some(info)) => Ok(format!("Active: {} on {}", info.ssid, info.interface)),
            Ok(None) => Ok("Inactive".into()),
            Err(e) => Err(zbus::fdo::Error::Failed(e.to_string())),
        }
    }

    async fn get_interfaces(&self) -> zbus::fdo::Result<Vec<String>> {
        match self.nm.get_all_interfaces().await {
            Ok(interfaces) => {
                let names: Vec<String> = interfaces.iter().map(|i| i.name.clone()).collect();
                Ok(names)
            }
            Err(e) => Err(zbus::fdo::Error::Failed(e.to_string())),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let nm = NmManager::new().await;
    let dbus = NimbusDbus { nm };

    let _conn = Builder::system()?
        .name("com.nimbus.Hotspot")?
        .serve_at("/com/nimbus/Hotspot", dbus)?
        .build()
        .await?;

    log::info!("Nimbus D-Bus service running. Press Ctrl+C to exit.");

    std::future::pending::<()>().await;

    Ok(())
}
