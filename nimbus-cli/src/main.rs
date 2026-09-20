use std::path::PathBuf;

use clap::{Parser, Subcommand};

use nimbus_backend::planner::plan_hotspot;
use nimbus_backend::shared_ap::SharedAp;
use nimbus_core::error::NimbusError;
use nimbus_core::types::{Band, HotspotBackend, HotspotConfig, HotspotPlan, Security, StaImpact};
use nimbus_network::manager::NmManager;
use nimbus_network::regdomain::RegDomainGuard;
use nimbus_network::traits::NetworkManagerApi;

/// Name of the pid file used by a detached shared hotspot.
const PID_FILE: &str = "nimbus-hotspot.pid";

#[derive(Parser)]
#[command(name = "nimbus")]
#[command(about = "Nimbus Hotspot - Modern Wi-Fi hotspot manager for Linux")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start a hotspot.
    Start {
        #[arg(short, long, default_value = "Nimbus-Hotspot")]
        ssid: String,
        /// WPA password. Prompted for (hidden) when omitted.
        #[arg(short, long)]
        password: Option<String>,
        #[arg(short, long, value_enum, default_value = "auto")]
        band: BandArg,
        #[arg(short, long)]
        channel: Option<u32>,
        #[arg(short = 'H', long)]
        hidden: bool,
        /// Wi-Fi adapter to use. Defaults to the one least likely to interrupt
        /// this machine's own connection.
        #[arg(short, long)]
        interface: Option<String>,
        /// Maximum number of connected devices. Omit for no limit.
        #[arg(short = 'm', long)]
        max_clients: Option<u32>,
        /// Two-letter regulatory domain (e.g. FR) to apply while the hotspot
        /// runs. Applies to every adapter on the machine and is restored on
        /// exit. Omit to leave the system setting alone.
        #[arg(long)]
        country: Option<String>,
        /// Go ahead even if starting the hotspot disconnects this machine's
        /// Wi-Fi.
        #[arg(short = 'y', long)]
        yes: bool,
        /// Leave the hotspot running after this command exits.
        #[arg(short = 'd', long)]
        detach: bool,
        /// Internal: marks the background process spawned by `--detach`.
        #[arg(long, hide = true)]
        foreground: bool,
    },
    /// Show what `start` would do, without changing anything.
    Plan {
        #[arg(short, long, default_value = "Nimbus-Hotspot")]
        ssid: String,
        #[arg(short, long, value_enum, default_value = "auto")]
        band: BandArg,
        #[arg(short, long)]
        channel: Option<u32>,
        #[arg(short, long)]
        interface: Option<String>,
        #[arg(long)]
        country: Option<String>,
    },
    /// Stop the active hotspot.
    Stop,
    /// Show hotspot status and connected clients.
    Status,
    /// List available Wi-Fi adapters.
    Devices,
    /// List the devices connected to the hotspot.
    Stations,
    /// Scan for nearby networks.
    Scan,
    /// List all network interfaces.
    Interfaces,
    /// Show a scannable QR code for the current (or a new) hotspot.
    Qr {
        /// SSID to encode. Defaults to the running hotspot.
        #[arg(short, long)]
        ssid: Option<String>,
        /// Password to encode. Read from the running hotspot profile when
        /// possible, otherwise prompted for.
        #[arg(short, long)]
        password: Option<String>,
    },
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

struct ConfigArgs {
    ssid: String,
    password: String,
    band: BandArg,
    channel: Option<u32>,
    hidden: bool,
    max_clients: Option<u32>,
    country: Option<String>,
}

fn build_config(args: ConfigArgs) -> HotspotConfig {
    HotspotConfig {
        ssid: args.ssid,
        password: args.password,
        band: args.band.into(),
        channel: args.channel,
        security: Security::Wpa2Wpa3Transition,
        hidden: args.hidden,
        max_clients: args.max_clients,
        country_code: args.country.map(|c| c.trim().to_ascii_uppercase()),
        ..Default::default()
    }
}

fn print_plan(plan: &HotspotPlan) {
    println!("Adapter:  {}", plan.ap_interface);
    println!(
        "Upstream: {}",
        plan.upstream_interface.as_deref().unwrap_or("none")
    );
    println!(
        "Backend:  {}{}",
        plan.backend,
        match plan.channel {
            Some(channel) => format!(" on channel {}", channel),
            None => String::new(),
        }
    );
    println!(
        "Devices:  {}",
        plan.effective_config
            .max_clients
            .map(|n| n.to_string())
            .unwrap_or_else(|| "unlimited".into())
    );
    if let Some(change) = &plan.regdomain_change {
        println!(
            "Country:  {} -> {} (machine-wide, restored on stop)",
            change.from.as_deref().unwrap_or("unset"),
            change.to
        );
    }
    match &plan.impact {
        StaImpact::None => println!("Impact:   your Wi-Fi connection is unaffected"),
        StaImpact::DisconnectsUplink { ssid } => println!(
            "Impact:   DISCONNECTS you from {}",
            ssid.as_deref().unwrap_or("your Wi-Fi network")
        ),
    }
    for warning in &plan.warnings {
        println!("Warning:  {}", warning);
    }
}

/// Asks for the WPA password without echoing it, twice when interactive.
fn prompt_password() -> Option<String> {
    let first = rpassword::prompt_password("Password: ").ok()?;
    let first = first.trim().to_string();
    if first.is_empty() {
        return None;
    }
    let second = rpassword::prompt_password("Confirm password: ").ok()?;
    if first != second.trim() {
        eprintln!("Passwords do not match");
        return None;
    }
    Some(first)
}

fn pid_path() -> PathBuf {
    if let Ok(runtime) = std::env::var("XDG_RUNTIME_DIR") {
        return PathBuf::from(runtime).join(PID_FILE);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".cache/nimbus").join(PID_FILE)
}

fn write_pid_file() {
    let path = pid_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, std::process::id().to_string());
}

fn read_pid_file() -> Option<i32> {
    std::fs::read_to_string(pid_path())
        .ok()
        .and_then(|raw| raw.trim().parse().ok())
}

fn remove_pid_file() {
    let _ = std::fs::remove_file(pid_path());
}

/// Waits until Ctrl+C or a `nimbus stop` SIGTERM arrives.
async fn wait_for_termination() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        if let Ok(mut term) = signal(SignalKind::terminate()) {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = term.recv() => {}
            }
            return;
        }
    }
    tokio::signal::ctrl_c().await.ok();
}

/// Runs the shared hotspot in the foreground and tears it down on Ctrl+C or
/// SIGTERM.
async fn run_shared(
    nm: &NmManager,
    plan: &HotspotPlan,
    guard: &mut RegDomainGuard,
) -> nimbus_core::Result<()> {
    let Some(channel) = plan.channel.or(plan.effective_config.channel) else {
        return Err(NimbusError::ConfigError(
            "sharing needs a channel to match your Wi-Fi connection".into(),
        ));
    };
    let Some(upstream) = plan.upstream_interface.clone() else {
        return Err(NimbusError::NoUpstreamInterface);
    };

    let base_mac = nm
        .get_wifi_devices()
        .await?
        .into_iter()
        .find(|d| d.name == plan.ap_interface)
        .map(|d| d.mac)
        .ok_or_else(|| NimbusError::InterfaceNotFound(plan.ap_interface.clone()))?;

    let ap = SharedAp::start(
        &plan.effective_config,
        &plan.ap_interface,
        base_mac,
        channel,
        &upstream,
    )
    .await?;

    write_pid_file();

    println!(
        "\nHotspot '{}' started on {} (channel {})",
        plan.effective_config.ssid,
        ap.interface(),
        channel
    );
    println!("  Gateway: {}", nimbus_backend::dhcp::GATEWAY);
    println!("  Sharing: {}", upstream);
    println!("  Press Ctrl+C to stop.");

    wait_for_termination().await;
    ap.shutdown().await;
    remove_pid_file();
    guard.restore().await?;
    println!("Hotspot stopped");
    Ok(())
}

/// Re-runs this command in the background for a shared hotspot, which has to
/// keep a process alive to own hostapd, dnsmasq and the NAT rules.
fn spawn_detached() -> nimbus_core::Result<()> {
    let exe = std::env::current_exe()?;
    let args: Vec<String> = std::env::args()
        .skip(1)
        .filter(|arg| arg != "--detach" && arg != "-d")
        .collect();

    let mut command = std::process::Command::new(exe);
    command
        .args(&args)
        .arg("--foreground")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Its own process group, so closing the terminal does not signal it.
        command.process_group(0);
    }

    let child = command.spawn()?;

    // The child writes the pid file only once the access point is actually up,
    // so its presence is the success signal.
    for _ in 0..50 {
        if read_pid_file() == Some(child.id() as i32) {
            println!(
                "Hotspot started in the background (pid {}). Use `nimbus stop` to stop it.",
                child.id()
            );
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }

    Err(NimbusError::HotspotCreationFailed(
        "the background process did not come up; run without --detach to see why".into(),
    ))
}

/// Reads the PSK of the Nimbus profile that is currently running, if nmcli can
/// tell us.
async fn active_psk(ssid: &str) -> Option<String> {
    let profile = format!("{}{}", nimbus_network::manager::PROFILE_PREFIX, ssid);
    let output = tokio::process::Command::new("nmcli")
        .args([
            "-s",
            "-g",
            "802-11-wireless-security.psk",
            "connection",
            "show",
            &profile,
        ])
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }
    let psk = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!psk.is_empty()).then_some(psk)
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    env_logger::init();
    let cli = Cli::parse();
    let nm = NmManager::new().await;

    let result = match cli.command {
        Commands::Start {
            ssid,
            password,
            band,
            channel,
            hidden,
            interface,
            max_clients,
            country,
            yes,
            detach,
            foreground,
        } => {
            let password = match password {
                Some(password) => password,
                None => match prompt_password() {
                    Some(password) => password,
                    None => {
                        eprintln!("A WPA password of at least 8 characters is required");
                        std::process::exit(2);
                    }
                },
            };

            let config = build_config(ConfigArgs {
                ssid,
                password,
                band,
                channel,
                hidden,
                max_clients,
                country,
            });

            // Work out the consequences before changing anything.
            let plan = match plan_hotspot(&nm, &config, interface.as_deref()).await {
                Ok(plan) => plan,
                Err(e) => {
                    eprintln!("Cannot start hotspot: {}", e);
                    std::process::exit(1);
                }
            };
            print_plan(&plan);

            if plan.needs_confirmation() && !yes {
                eprintln!(
                    "\nRefusing to continue: this would disconnect you. \
                     Re-run with --yes to go ahead."
                );
                std::process::exit(1);
            }

            if detach
                && !foreground
                && plan.backend == HotspotBackend::NetworkManager
                && plan.regdomain_change.is_some()
            {
                eprintln!(
                    "--country cannot be used with --detach: the regulatory domain \
                     must be put back when the hotspot stops, which needs a \
                     process to stay alive. Run in the foreground, or set the \
                     country system-wide."
                );
                std::process::exit(1);
            }

            if detach && !foreground && plan.backend == HotspotBackend::SharedVirtualAp {
                return match spawn_detached() {
                    Ok(()) => std::process::ExitCode::SUCCESS,
                    Err(e) => {
                        eprintln!("Could not start in the background: {}", e);
                        std::process::ExitCode::FAILURE
                    }
                };
            }

            // A regulatory-domain switch applies to the whole machine, so it
            // is applied here and put back when the hotspot stops.
            let mut guard = RegDomainGuard::new();
            if let Some(change) = &plan.regdomain_change {
                if let Err(e) = guard.apply(&change.to).await {
                    eprintln!("Could not set the country: {}", e);
                    std::process::exit(1);
                }
            }

            match plan.backend {
                HotspotBackend::SharedVirtualAp => {
                    if let Err(e) = run_shared(&nm, &plan, &mut guard).await {
                        eprintln!("Failed to start the shared hotspot: {}", e);
                        let _ = guard.restore().await;
                        Err(e)
                    } else {
                        Ok(())
                    }
                }
                HotspotBackend::NetworkManager => {
                    match nm
                        .create_hotspot(&plan.effective_config, &plan.ap_interface)
                        .await
                    {
                        Ok(info) => {
                            println!("\nHotspot '{}' started on {}", info.ssid, info.interface);
                            println!("  IP: {}", info.ip);
                            println!("  Channel: {} ({} MHz)", info.channel, info.frequency);
                            if detach {
                                if let Err(e) = guard.restore().await {
                                    eprintln!("Could not restore the country: {}", e);
                                }
                                println!("  Running in the background.");
                            } else {
                                println!("  Press Ctrl+C to stop");
                                wait_for_termination().await;
                                let stop = nm.stop_hotspot().await;
                                if let Err(e) = guard.restore().await {
                                    eprintln!("Could not restore the country: {}", e);
                                }
                                match stop {
                                    Ok(()) => println!("Hotspot stopped"),
                                    Err(e) => eprintln!("Failed to stop hotspot: {}", e),
                                }
                            }
                            Ok(())
                        }
                        Err(e) => {
                            eprintln!("Failed to start hotspot: {}", e);
                            let _ = guard.restore().await;
                            Err(e)
                        }
                    }
                }
            }
        }
        Commands::Plan {
            ssid,
            band,
            channel,
            interface,
            country,
        } => {
            // Planning never creates a connection; a throwaway password that
            // satisfies validation keeps the command free of prompts.
            let config = build_config(ConfigArgs {
                ssid,
                password: "00000000".to_string(),
                band,
                channel,
                hidden: false,
                max_clients: None,
                country,
            });
            match plan_hotspot(&nm, &config, interface.as_deref()).await {
                Ok(plan) => {
                    print_plan(&plan);
                    Ok(())
                }
                Err(e) => {
                    eprintln!("Cannot plan hotspot: {}", e);
                    Err(e)
                }
            }
        }
        Commands::Stop => {
            let mut stopped_shared = false;
            if let Some(pid) = read_pid_file() {
                #[cfg(unix)]
                let alive = unsafe { libc::kill(pid, 0) } == 0;
                #[cfg(not(unix))]
                let alive = true;

                if alive {
                    #[cfg(unix)]
                    unsafe {
                        libc::kill(pid, libc::SIGTERM);
                    }
                    // Give the background process time to tear down hostapd,
                    // dnsmasq and the NAT rules.
                    for _ in 0..50 {
                        if read_pid_file().is_none() {
                            stopped_shared = true;
                            break;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    }
                }
                // Either it stopped, or the pid file was stale.
                remove_pid_file();
            }

            match nm.stop_hotspot().await {
                Ok(()) => {
                    println!("Hotspot stopped");
                    Ok(())
                }
                Err(NimbusError::HotspotNotActive) if stopped_shared => {
                    println!("Hotspot stopped");
                    Ok(())
                }
                Err(NimbusError::HotspotNotActive) => {
                    println!("No active hotspot");
                    Ok(())
                }
                Err(e) => {
                    eprintln!("Failed to stop hotspot: {}", e);
                    Err(e)
                }
            }
        }
        Commands::Status => match nm.get_active_hotspot().await {
            Ok(Some(info)) => {
                println!("Hotspot active");
                println!("  SSID: {}", info.ssid);
                println!("  Interface: {}", info.interface);
                println!("  IP: {}", info.ip);
                println!("  Frequency: {} MHz", info.frequency);
                match nm.get_connected_stations(&info.interface).await {
                    Ok(stations) if !stations.is_empty() => {
                        println!("  Devices: {}", stations.len());
                        for station in stations {
                            println!(
                                "    {} - {} dBm - ↑ {} - ↓ {}",
                                station.mac,
                                station.signal_dbm,
                                nimbus_telemetry::manufacturer::format_bytes(station.tx_bytes),
                                nimbus_telemetry::manufacturer::format_bytes(station.rx_bytes),
                            );
                        }
                    }
                    Ok(_) => println!("  Devices: 0"),
                    Err(e) => println!("  Devices: unavailable ({})", e),
                }
                Ok(())
            }
            Ok(None) => {
                println!("No active hotspot");
                Ok(())
            }
            Err(e) => {
                eprintln!("Failed to get status: {}", e);
                Err(e)
            }
        },
        Commands::Devices => match nm.get_wifi_devices().await {
            Ok(devices) if devices.is_empty() => {
                println!("No Wi-Fi adapter found");
                Ok(())
            }
            Ok(devices) => {
                println!("Wi-Fi adapters:");
                for device in devices {
                    println!(
                        "  {} - {} - {:?} - {}",
                        device.name,
                        device.mac,
                        device.state,
                        if device.driver.is_empty() {
                            "unknown driver"
                        } else {
                            &device.driver
                        }
                    );
                }
                Ok(())
            }
            Err(e) => {
                eprintln!("Failed to list adapters: {}", e);
                Err(e)
            }
        },
        Commands::Stations => {
            let interface = match nm.get_active_hotspot().await {
                Ok(Some(info)) => info.interface,
                _ => {
                    eprintln!("No active hotspot");
                    std::process::exit(1);
                }
            };
            match nm.get_connected_stations(&interface).await {
                Ok(stations) => {
                    println!("Connected devices: {}", stations.len());
                    for s in &stations {
                        println!(
                            "  {} - Signal: {} dBm - ↑ {} - ↓ {}",
                            s.mac,
                            s.signal_dbm,
                            nimbus_telemetry::manufacturer::format_bytes(s.tx_bytes),
                            nimbus_telemetry::manufacturer::format_bytes(s.rx_bytes),
                        );
                    }
                    Ok(())
                }
                Err(e) => {
                    eprintln!("Failed to get devices: {}", e);
                    Err(e)
                }
            }
        }
        Commands::Scan => match nm.get_wifi_devices().await {
            Ok(devices) => match devices.first() {
                Some(wifi) => match nimbus_wifi::scanner::scan_available_networks(&wifi.name).await
                {
                    Ok(networks) => {
                        println!("Found {} networks:", networks.len());
                        for n in &networks {
                            println!(
                                "  {} ({}) - Ch {} - {} dBm",
                                if n.ssid.is_empty() {
                                    "<hidden>"
                                } else {
                                    &n.ssid
                                },
                                n.bssid,
                                n.channel,
                                n.signal_dbm
                            );
                        }
                        Ok(())
                    }
                    Err(e) => {
                        eprintln!("Scan failed: {}", e);
                        Err(e)
                    }
                },
                None => {
                    println!("No WiFi adapter found");
                    Ok(())
                }
            },
            Err(e) => {
                eprintln!("Failed to list adapters: {}", e);
                Err(e)
            }
        },
        Commands::Interfaces => match nm.get_all_interfaces().await {
            Ok(interfaces) => {
                println!("Network interfaces:");
                for iface in &interfaces {
                    println!(
                        "  {} ({:?}) - {:?} - {}",
                        iface.name, iface.interface_type, iface.state, iface.mac
                    );
                }
                Ok(())
            }
            Err(e) => {
                eprintln!("Failed to list interfaces: {}", e);
                Err(e)
            }
        },
        Commands::Qr { ssid, password } => {
            let active = nm.get_active_hotspot().await.ok().flatten();

            let ssid = match ssid {
                Some(ssid) => ssid,
                None => match &active {
                    Some(info) => info.ssid.clone(),
                    None => {
                        eprintln!("No active hotspot: pass --ssid (and --password)");
                        std::process::exit(1);
                    }
                },
            };

            let password = match password {
                Some(password) => password,
                None => {
                    let from_running = active.as_ref().is_some_and(|info| info.ssid == ssid);
                    let psk = if from_running {
                        active_psk(&ssid).await
                    } else {
                        None
                    };
                    match psk {
                        Some(psk) => psk,
                        None => prompt_password().unwrap_or_default(),
                    }
                }
            };

            match nimbus_wifi::qr::generate_wifi_qr_unicode(
                &ssid,
                &password,
                &Security::Wpa2Wpa3Transition,
                false,
            ) {
                Ok(qr) => {
                    println!("Scan to join '{}':", ssid);
                    println!("{}", qr);
                    Ok(())
                }
                Err(e) => {
                    eprintln!("Could not generate the QR code: {}", e);
                    Err(e)
                }
            }
        }
    };

    if result.is_err() {
        std::process::ExitCode::FAILURE
    } else {
        std::process::ExitCode::SUCCESS
    }
}
