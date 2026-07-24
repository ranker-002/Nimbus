use clap::{Parser, Subcommand};

use nimbus_core::types::{Band, HotspotConfig, Security};
use nimbus_network::manager::NmManager;
use nimbus_network::traits::NetworkManagerApi;

#[derive(Parser)]
#[command(name = "nimbus")]
#[command(about = "Nimbus Hotspot - Modern Wi-Fi hotspot manager for Linux")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Start {
        #[arg(short, long, default_value = "Nimbus-Hotspot")]
        ssid: String,
        #[arg(short, long)]
        password: Option<String>,
        #[arg(short, long, value_enum, default_value = "auto")]
        band: BandArg,
        #[arg(short, long)]
        channel: Option<u32>,
        #[arg(short = 'H', long)]
        hidden: bool,
    },
    Stop,
    Status,
    Devices,
    Scan,
    Interfaces,
}

#[derive(clap::ValueEnum, Clone)]
enum BandArg {
    Auto,
    Band24,
    Band5,
}

impl From<BandArg> for Band {
    fn from(arg: BandArg) -> Self {
        match arg {
            BandArg::Auto => Band::Auto,
            BandArg::Band24 => Band::Band2_4Ghz,
            BandArg::Band5 => Band::Band5Ghz,
        }
    }
}

#[tokio::main]
async fn main() {
    env_logger::init();
    let cli = Cli::parse();
    let nm = NmManager::new().await;

    match cli.command {
        Commands::Start {
            ssid,
            password,
            band,
            channel,
            hidden,
        } => {
            let password = password.unwrap_or_else(|| {
                println!("Enter password: ");
                let mut input = String::new();
                std::io::stdin().read_line(&mut input).unwrap();
                input.trim().to_string()
            });

            let config = HotspotConfig {
                ssid: ssid.clone(),
                password,
                band: band.into(),
                channel,
                country_code: "US".to_string(),
                security: Security::Wpa2Wpa3Transition,
                hidden,
                max_clients: Some(10),
                client_isolation: false,
                ipv4_method: nimbus_core::types::Ipv4Method::Shared,
                auto_start: false,
            };

            let _upstream = nm.get_upstream_interface().await.unwrap_or(None).unwrap_or_else(|| {
                eprintln!("No upstream interface found");
                std::process::exit(1);
            });

            let wifi_devices = nm.get_wifi_devices().await.unwrap_or_else(|e| {
                eprintln!("Failed to get WiFi devices: {}", e);
                std::process::exit(1);
            });

            let interface = wifi_devices.first().map(|d| d.name.clone()).unwrap_or_else(|| {
                eprintln!("No WiFi adapter found");
                std::process::exit(1);
            });

            match nm.create_hotspot(&config, &interface).await {
                Ok(info) => {
                    println!("✓ Hotspot '{}' started on {}", info.ssid, info.interface);
                    println!("  IP: {}", info.ip);
                    println!("  Press Ctrl+C to stop");

                    tokio::signal::ctrl_c().await.ok();
                    let _ = nm.stop_hotspot().await;
                    println!("✓ Hotspot stopped");
                }
                Err(e) => {
                    eprintln!("✗ Failed to start hotspot: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::Stop => match nm.stop_hotspot().await {
            Ok(()) => println!("✓ Hotspot stopped"),
            Err(e) => {
                eprintln!("✗ Failed to stop hotspot: {}", e);
                std::process::exit(1);
            }
        },
        Commands::Status => match nm.get_active_hotspot().await {
            Ok(Some(info)) => {
                println!("✓ Hotspot active");
                println!("  SSID: {}", info.ssid);
                println!("  Interface: {}", info.interface);
                println!("  IP: {}", info.ip);
                println!("  Frequency: {} MHz", info.frequency);
            }
            Ok(None) => println!("No active hotspot"),
            Err(e) => {
                eprintln!("✗ Failed to get status: {}", e);
                std::process::exit(1);
            }
        },
        Commands::Devices => {
            let interfaces = nm.get_all_interfaces().await.unwrap_or_default();
            let wifi = interfaces.iter().find(|i| i.interface_type == nimbus_core::types::InterfaceType::Wifi);
            if let Some(wifi) = wifi {
                match nm.get_connected_stations(&wifi.name).await {
                    Ok(stations) => {
                        println!("Connected devices: {}", stations.len());
                        for s in &stations {
                            println!(
                                "  {} - Signal: {} dBm, ↑{} ↓{}",
                                s.mac,
                                s.signal_dbm,
                                nimbus_telemetry::manufacturer::format_bytes(s.tx_bytes),
                                nimbus_telemetry::manufacturer::format_bytes(s.rx_bytes),
                            );
                        }
                    }
                    Err(e) => eprintln!("✗ Failed to get devices: {}", e),
                }
            } else {
                println!("No WiFi adapter found");
            }
        }
        Commands::Scan => {
            let interfaces = nm.get_all_interfaces().await.unwrap_or_default();
            let wifi = interfaces.iter().find(|i| i.interface_type == nimbus_core::types::InterfaceType::Wifi);
            if let Some(wifi) = wifi {
                match nimbus_wifi::scanner::scan_available_networks(&wifi.name).await {
                    Ok(networks) => {
                        println!("Found {} networks:", networks.len());
                        for n in &networks {
                            println!(
                                "  {} ({}) - Ch {} - {} dBm",
                                n.ssid, n.bssid, n.channel, n.signal_dbm
                            );
                        }
                    }
                    Err(e) => eprintln!("✗ Scan failed: {}", e),
                }
            }
        }
        Commands::Interfaces => {
            match nm.get_all_interfaces().await {
                Ok(interfaces) => {
                    println!("Network interfaces:");
                    for iface in &interfaces {
                        println!(
                            "  {} ({:?}) - {:?} - {}",
                            iface.name, iface.interface_type, iface.state, iface.mac
                        );
                    }
                }
                Err(e) => eprintln!("✗ Failed to list interfaces: {}", e),
            }
        }
    }
}
