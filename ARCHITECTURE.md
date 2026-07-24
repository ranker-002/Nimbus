# Nimbus Hotspot — Architecture

## 1. Executive Summary

Nimbus Hotspot is a native Linux application for creating and managing Wi-Fi hotspots. It replaces legacy tools like `create_ap` with a modern, modular Rust/GTK4/Libadwaita stack that communicates exclusively via NetworkManager's D-Bus API, `iw`/nl80211 for capability detection, and nftables for firewall/NAT.

**Core design principle**: NetworkManager does 90% of the heavy lifting. Nimbus is an orchestrator and UI, not a reimplementer of NM internals.

---

## 2. Technology Choices & Rationale

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Language | Rust | Memory safety, zero-cost abstractions, excellent async ecosystem |
| Async runtime | Tokio (background) + GLib main loop (UI) | Tokio for I/O-bound backend; GLib for GTK event loop. Never `#[tokio::main]` |
| IPC to NM | zbus 5.x (via `nmrs` crate) | nmrs wraps NM D-Bus API with async Rust. Battle-tested in Pop!_OS COSMIC |
| UI framework | GTK4 + Libadwaita | Native GNOME integration, adaptive layouts, dark mode built-in |
| UI pattern | MVPVM (Model-View-Presenter-ViewModel) | Clean separation, testable model layer, property binding |
| Firewall | nftables (via `nft` CLI or direct netlink) | Modern replacement for iptables, used by NM 1.30+ |
| QR codes | `qrcode` crate | Pure Rust, SVG/PNG output, no external deps |
| Config | GSettings (via `gio::Settings`) | Standard GNOME config, auto-persisted, D-Bus accessible |
| Build | Meson + Cargo | Meson for Flatpak/system install, Cargo for Rust build |
| Package | Flatpak (primary), distro packages | Covers all target distros |
| Crate for NM | `nmrs` 3.4+ | High-level, async-first, used by COSMIC desktop |
| Capabilities | `iw list` parsing + nl80211 netlink | Reliable detection of AP mode, WPA3, WiFi 6/6E/7 |
| DHCP/DNS | NM internal dnsmasq (via `ipv4.method=shared`) | Zero config — NM spawns dnsmasq automatically |
| NAT | NM auto-configured + nftables rules | NM sets up masquerade when `ipv4.method=shared` |

---

## 3. Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    Nimbus Hotspot                            │
│                                                             │
│  ┌─────────────────────────────────────────────────────┐    │
│  │                   Frontend (GTK4)                    │    │
│  │  ┌─────────┐ ┌──────────┐ ┌────────┐ ┌──────────┐  │    │
│  │  │Dashboard │ │ Hotspot  │ │  Settings │ │  Devices │  │    │
│  │  │  Page    │ │  Page    │ │  Page    │ │  Page    │  │    │
│  │  └────┬────┘ └────┬─────┘ └────┬────┘ └────┬─────┘  │    │
│  │       └───────────┼───────────┼────────────┘         │    │
│  │              ┌────▼────┐                               │    │
│  │              │Presenter│ (MVVM glue)                   │    │
│  │              └────┬────┘                               │    │
│  └───────────────────┼──────────────────────────────────┘    │
│                      │ async-channel                          │
│  ┌───────────────────┼──────────────────────────────────┐    │
│  │              ┌────▼────┐                               │    │
│  │              │  Model  │ (Business logic)              │    │
│  │              └────┬────┘                               │    │
│  │         ┌─────────┼──────────┐                         │    │
│  │    ┌────▼───┐ ┌───▼────┐ ┌──▼──────┐                  │    │
│  │    │ Network│ │  Wifi  │ │Settings │                  │    │
│  │    │ Module │ │ Module │ │ Module  │                  │    │
│  │    └───┬────┘ └───┬────┘ └─────────┘                  │    │
│  └────────┼──────────┼───────────────────────────────────┘    │
│           │          │                                        │
│  ┌────────┼──────────┼───────────────────────────────────┐    │
│  │   Backend Services (async)                            │    │
│  │    ┌────▼──────────▼────┐                              │    │
│  │    │   nmrs (D-Bus)     │                              │    │
│  │    └────────┬───────────┘                              │    │
│  │    ┌────────▼───────────┐  ┌──────────────────┐       │    │
│  │    │  iw/nl80211 detect │  │  nftables manager│       │    │
│  │    └────────────────────┘  └──────────────────┘       │    │
│  └───────────────────────────────────────────────────────┘    │
│                      │                                        │
│  ┌───────────────────┼───────────────────────────────────┐    │
│  │              Linux System                              │    │
│  │    NetworkManager (D-Bus)  ·  iw  ·  nftables         │    │
│  │    hostapd (via NM)  ·  dnsmasq (via NM)              │    │
│  └───────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────┘
```

---

## 4. Module Architecture

### 4.1 Core Layer (`core/`)

Shared types, error handling, runtime utilities.

```
core/
├── mod.rs
├── error.rs          # NimbusError enum, Result type alias
├── runtime.rs        # Tokio runtime singleton (OnceLock<Runtime>)
├── types.rs          # Shared types: MacAddress, IpInfo, Band, Security, etc.
├── events.rs         # Event enum for Model -> Presenter communication
└── constants.rs      # APP_ID, VERSION, default values
```

**Key types** (`core/types.rs`):

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotspotConfig {
    pub ssid: String,
    pub password: String,
    pub band: Band,
    pub channel: Option<u32>,
    pub country_code: String,
    pub security: Security,
    pub hidden: bool,
    pub max_clients: Option<u32>,
    pub client_isolation: bool,
    pub ipv4_method: Ipv4Method,
    pub auto_start: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Band {
    Band2_4Ghz,
    Band5Ghz,
    Auto,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Security {
    Wpa2,
    Wpa3,
    Wpa2Wpa3Transition,
    Open,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotspotState {
    Inactive,
    Starting,
    Active(HotspotInfo),
    Stopping,
    Error(String),
}

#[derive(Debug, Clone)]
pub struct HotspotInfo {
    pub interface: String,
    pub ssid: String,
    pub ip: Ipv4Addr,
    pub frequency: u32,
    pub channel: u32,
    pub started_at: Instant,
}

#[derive(Debug, Clone)]
pub struct StationInfo {
    pub mac: MacAddress,
    pub ip: Option<Ipv4Addr>,
    pub hostname: Option<String>,
    pub manufacturer: Option<String>,
    pub signal_strength: i32,    // dBm
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub connected_since: DateTime<Utc>,
    pub rx_rate: u32,            // Mbit/s
    pub tx_rate: u32,            // Mbit/s
}

#[derive(Debug, Clone)]
pub struct AdapterCapabilities {
    pub interface: String,
    pub phy_name: String,
    pub driver: String,
    pub supports_ap: bool,
    pub supports_wpa3: bool,
    pub supports_wifi_6: bool,      // 802.11ax (HE)
    pub supports_wifi_6e: bool,     // 6GHz band
    pub supports_wifi_7: bool,      // 802.11be (EHT)
    pub supports_simultaneous_sta_ap: bool,
    pub supported_bands: Vec<Band>,
    pub supported_channels_2ghz: Vec<u32>,
    pub supported_channels_5ghz: Vec<u32>,
    pub max_sta: u32,
}
```

### 4.2 Network Module (`network/`)

All NetworkManager D-Bus interactions. Abstracts NM behind clean Rust traits.

```
network/
├── mod.rs
├── manager.rs        # NetworkManager facade (nmrs wrapper)
├── connection.rs     # Create/modify/delete NM connections
├── hotspot.rs        # Hotspot-specific NM operations
├── monitor.rs        # Active connection monitoring
├── station.rs        # Connected client detection (iw + D-Bus)
├── interface.rs      # Network interface detection & enumeration
└── traits.rs         # NetworkManager trait for testability
```

**Key interface** (`network/traits.rs`):

```rust
#[async_trait]
pub trait NetworkManagerApi: Send + Sync {
    async fn get_wifi_devices(&self) -> Result<Vec<WifiDevice>>;
    async fn get_adapter_capabilities(&self, interface: &str) -> Result<AdapterCapabilities>;
    async fn create_hotspot(&self, config: &HotspotConfig, interface: &str) -> Result<String>;
    async fn stop_hotspot(&self, connection_path: &str) -> Result<()>;
    async fn get_active_hotspot(&self) -> Result<Option<HotspotInfo>>;
    async fn get_connected_stations(&self, interface: &str) -> Result<Vec<StationInfo>>;
    async fn get_upstream_interface(&self) -> Result<Option<String>>;
    async fn monitor_changes(&self, sender: Sender<NetworkEvent>) -> Result<()>;
}
```

**NM connection creation** (`network/hotspot.rs`):

The core operation uses `nmrs::builders::WifiConnectionBuilder`:

```rust
pub async fn build_hotspot_connection(
    config: &HotspotConfig,
    interface: &str,
) -> Result<ConnectionSettings> {
    let mut builder = WifiConnectionBuilder::new(&config.ssid)
        .mode(WifiMode::Ap)
        .wpa_psk(&config.password)
        .ipv4_shared()          // NM auto-configures DHCP + NAT
        .ipv6_ignore();

    builder = match config.band {
        Band::Band2_4Ghz => builder.band(WifiBand::Bg),
        Band::Band5Ghz => builder.band(WifiBand::A),
        Band::Auto => builder, // Let NM choose
    };

    if let Some(ch) = config.channel {
        builder = builder.channel(ch);
    }

    if config.hidden {
        builder = builder.hidden(true);
    }

    if config.client_isolation {
        builder = builder.ap_isolation(true);
    }

    if let Some(max) = config.max_clients {
        builder = builder.max_sta(max);
    }

    builder.build()
}
```

**Station detection** (`network/station.rs`):

NM does NOT expose connected stations via D-Bus. We parse `iw dev <iface> station dump`:

```rust
pub async fn get_stations(interface: &str) -> Result<Vec<StationInfo>> {
    let output = Command::new("iw")
        .args(["dev", interface, "station", "dump"])
        .output()
        .await?;

    // Parse output: each station block starts with "Station <mac>"
    // Fields: signal avg, rx bytes, tx bytes, rx rate, tx rate, connected time
    parse_iw_station_dump(&String::from_utf8_lossy(&output.stdout))
}
```

### 4.3 WiFi Module (`wifi/`)

Hardware capability detection via `iw list` and nl80211.

```
wifi/
├── mod.rs
├── capabilities.rs   # Parse iw list output for adapter capabilities
├── scanner.rs        # Scan available networks
├── channel.rs        # Channel selection logic (auto/manual)
└── qr.rs             # QR code generation for Wi-Fi credentials
```

**Capability detection** (`wifi/capabilities.rs`):

```rust
pub async fn detect_capabilities(interface: &str) -> Result<AdapterCapabilities> {
    let phy = get_phy_name(interface).await?;
    let output = Command::new("iw")
        .args(["phy", &phy, "info"])
        .output()
        .await?;

    let info = parse_iw_phy_info(&String::from_utf8_lossy(&output.stdout))?;

    Ok(AdapterCapabilities {
        interface: interface.to_string(),
        phy_name: phy,
        driver: get_driver_name(interface).await?,
        supports_ap: info.supported_modes.contains(&"AP"),
        supports_wpa3: info.extended_features.iter().any(|f| f.contains("SAE")),
        supports_wifi_6: info.has_he_capabilities,
        supports_wifi_6e: info.supports_6ghz,
        supports_wifi_7: info.has_eht_capabilities,
        supports_simultaneous_sta_ap: info.can_do_sta_and_ap,
        supported_bands: info.supported_bands,
        supported_channels_2ghz: info.channels_2ghz,
        supported_channels_5ghz: info.channels_5ghz,
        max_sta: info.max_sta.unwrap_or(32),
    })
}
```

**QR code generation** (`wifi/qr.rs`):

```rust
pub fn generate_wifi_qr(
    ssid: &str,
    password: &str,
    security: &Security,
    hidden: bool,
) -> Result<String> {
    let auth_type = match security {
        Security::Wpa2 => "WPA",
        Security::Wpa3 => "SAE",
        Security::Wpa2Wpa3Transition => "WPA",
        Security::Open => "nopass",
    };

    let wifi_string = format!(
        "WIFI:T:{};S:{};P:{};H:{};;",
        auth_type, ssid, password, hidden
    );

    let code = QrCode::new(&wifi_string)?;
    let svg = code.render::<svg::Color>()
        .min_dimensions(200, 200)
        .build();

    Ok(svg)
}
```

### 4.4 Settings Module (`settings/`)

Persistent configuration via GSettings.

```
settings/
├── mod.rs
├── schema.rs         # GSettings schema definition
├── hotspots.rs       # Saved hotspot profiles
├── preferences.rs    # App preferences (theme, auto-start, etc.)
└── security.rs       # Password rotation, blacklist/whitelist
```

**GSettings schema** (`data/com.nimbus.Hotspot.gschema.xml`):

```xml
<schemalist>
  <schema id="com.nimbus.Hotspot" path="/com/nimbus/Hotspot/">
    <key name="saved-hotspots" type="aa{sv}">
      <default>[]</default>
      <summary>Saved hotspot configurations</summary>
    </key>
    <key name="auto-start" type="b">
      <default>false</default>
      <summary>Start hotspot on login</summary>
    </key>
    <key name="default-band" type="s">
      <default>'auto'</default>
      <summary>Default frequency band</summary>
    </key>
    <key name="default-security" type="s">
      <default>'wpa2-wpa3'</default>
      <summary>Default security mode</summary>
    </key>
    <key name="password-rotation" type="b">
      <default>false</default>
      <summary>Auto-rotate password on schedule</summary>
    </key>
    <key name="max-clients-default" type="u">
      <default>10</default>
      <summary>Default max clients per hotspot</summary>
    </key>
  </schema>
</schemalist>
```

### 4.5 Telemetry Module (`telemetry/`)

Real-time monitoring and statistics.

```
telemetry/
├── mod.rs
├── collector.rs      # Gather stats from NM + iw
├── history.rs        # Connection history (SQLite via rusqlite)
├── bandwidth.rs      # Upload/download rate calculation
└── manufacturer.rs   # MAC OUI lookup for device manufacturer
```

**Bandwidth tracking** (`telemetry/bandwidth.rs`):

```rust
pub struct BandwidthTracker {
    interface: String,
    last_rx: u64,
    last_tx: u64,
    last_instant: Instant,
}

impl BandwidthTracker {
    pub fn sample(&mut self) -> BandwidthSample {
        let current_rx = read_sysfs_counter(&self.interface, "rx_bytes");
        let current_tx = read_sysfs_counter(&self.interface, "tx_bytes");
        let now = Instant::now();

        let elapsed = now.duration_since(self.last_instant).as_secs_f64();
        let rx_rate = if elapsed > 0.0 {
            ((current_rx - self.last_rx) as f64 / elapsed) as u64
        } else { 0 };
        let tx_rate = if elapsed > 0.0 {
            ((current_tx - self.last_tx) as f64 / elapsed) as u64
        } else { 0 };

        self.last_rx = current_rx;
        self.last_tx = current_tx;
        self.last_instant = now;

        BandwidthSample { rx_rate, tx_rate, total_rx: current_rx, total_tx: current_tx }
    }
}
```

### 4.6 Backend Module (`backend/`)

Orchestration layer connecting model to system services.

```
backend/
├── mod.rs
├── orchestrator.rs   # High-level hotspot lifecycle management
├── firewall.rs       # nftables rule management
├── dns.rs            # dnsmasq configuration (fallback only)
└── captive.rs        # Captive portal management (optional)
```

### 4.7 UI Layer (`ui/`)

GTK4/Libadwaita frontend.

```
ui/
├── mod.rs
├── app.rs                # AdwApplication setup, CSS loading
├── window.rs             # Main AdwApplicationWindow
├── navigation.rs         # AdwNavigationView setup
├── styles.css            # Custom dark theme CSS
├── pages/
│   ├── mod.rs
│   ├── dashboard.rs      # Real-time monitoring dashboard
│   ├── hotspot.rs        # Create/edit hotspot form
│   ├── devices.rs        # Connected devices list
│   ├── settings.rs       # App settings
│   └── first_run.rs      # First-run assistant
├── components/
│   ├── mod.rs
│   ├── status_card.rs    # Hotspot status display
│   ├── qr_dialog.rs      # QR code popup
│   ├── station_row.rs    # Device list row widget
│   ├── channel_picker.rs # Channel selection widget
│   ├── band_picker.rs    # Band selection widget
│   └── capability_badge.rs # Adapter capability display
└── viewmodels/
    ├── mod.rs
    ├── dashboard_vm.rs   # Dashboard ViewModel (GObject properties)
    ├── hotspot_vm.rs     # Hotspot form ViewModel
    └── devices_vm.rs     # Devices list ViewModel
```

**UI Architecture Flow**:

```
User clicks "Start Hotspot"
    │
    ▼
View (hotspot.rs) ──connect_clicked──► Presenter
    │                                      │
    │                              on_start_clicked()
    │                                      │
    │                              ┌───────▼────────┐
    │                              │ Model.spawn()   │
    │                              │ (tokio runtime) │
    │                              └───────┬────────┘
    │                                      │
    │                              Backend::create_hotspot()
    │                                      │
    │                              nmrs D-Bus call
    │                                      │
    │                              Event::HotspotStarted(info)
    │                                      │
    │                              async_channel::send()
    │                                      │
    │                              ┌───────▼────────┐
    │                              │ glib::spawn     │
    │                              │ _future_local   │
    │                              └───────┬────────┘
    │                                      │
    ▼                                      ▼
ViewModel ◄──bind_property── View updates (status, IP, etc.)
```

---

## 5. Project Root Structure

```
nimbus-hotspot/
├── Cargo.toml                    # Workspace root
├── Cargo.lock
├── build.rs                      # GResource compilation
├── meson.build                   # Meson build (Flatpak)
├── meson_options.txt
├── README.md
├── LICENSE                       # GPL-3.0
├── SECURITY.md
├── CONTRIBUTING.md
│
├── nimbus-core/                  # Core types & utilities
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── error.rs
│       ├── runtime.rs
│       ├── types.rs
│       ├── events.rs
│       └── constants.rs
│
├── nimbus-network/               # NetworkManager D-Bus layer
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── manager.rs
│       ├── connection.rs
│       ├── hotspot.rs
│       ├── monitor.rs
│       ├── station.rs
│       ├── interface.rs
│       └── traits.rs
│
├── nimbus-wifi/                  # Wi-Fi capabilities & QR
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── capabilities.rs
│       ├── scanner.rs
│       ├── channel.rs
│       └── qr.rs
│
├── nimbus-settings/              # Configuration management
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── schema.rs
│       ├── hotspots.rs
│       └── preferences.rs
│
├── nimbus-telemetry/             # Monitoring & stats
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── collector.rs
│       ├── history.rs
│       ├── bandwidth.rs
│       └── manufacturer.rs
│
├── nimbus-backend/               # Orchestration & firewall
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── orchestrator.rs
│       ├── firewall.rs
│       ├── dns.rs
│       └── captive.rs
│
├── nimbus-ui/                    # GTK4/Libadwaita frontend
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs
│       ├── app.rs
│       ├── window.rs
│       ├── navigation.rs
│       ├── styles.css
│       ├── pages/
│       │   ├── mod.rs
│       │   ├── dashboard.rs
│       │   ├── hotspot.rs
│       │   ├── devices.rs
│       │   ├── settings.rs
│       │   └── first_run.rs
│       ├── components/
│       │   ├── mod.rs
│       │   ├── status_card.rs
│       │   ├── qr_dialog.rs
│       │   ├── station_row.rs
│       │   ├── channel_picker.rs
│       │   ├── band_picker.rs
│       │   └── capability_badge.rs
│       └── viewmodels/
│           ├── mod.rs
│           ├── dashboard_vm.rs
│           ├── hotspot_vm.rs
│           └── devices_vm.rs
│
├── nimbus-cli/                   # Optional CLI interface
│   ├── Cargo.toml
│   └── src/
│       └── main.rs
│
├── nimbus-dbus-api/              # D-Bus service (for external tools)
│   ├── Cargo.toml
│   └── src/
│       └── main.rs
│
├── data/                         # Desktop integration
│   ├── com.nimbus.Hotspot.desktop.in
│   ├── com.nimbus.Hotspot.metainfo.xml.in
│   ├── com.nimbus.Hotspot.gschema.xml
│   ├── resources.gresource.xml
│   ├── com.nimbus.Hotspot.css
│   └── icons/
│       └── hicolor/scalable/apps/
│           └── com.nimbus.Hotspot.svg
│
├── tests/                        # Integration tests
│   ├── integration/
│   │   ├── hotspot_lifecycle.rs
│   │   ├── station_monitoring.rs
│   │   └── firewall.rs
│   └── fixtures/
│       └── mock_nm.rs
│
└── .github/
    └── workflows/
        ├── ci.yml
        └── release.yml
```

---

## 6. Data Models

### 6.1 Hotspot Configuration (Persistent)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedHotspot {
    pub uuid: String,
    pub name: String,
    pub config: HotspotConfig,
    pub created_at: DateTime<Utc>,
    pub last_used: Option<DateTime<Utc>>,
    pub use_count: u32,
}
```

### 6.2 Network Interface

```rust
#[derive(Debug, Clone)]
pub struct NetworkInterface {
    pub name: String,              // e.g., "wlp3s0"
    pub interface_type: InterfaceType,
    pub mac: MacAddress,
    pub state: InterfaceState,
    pub driver: String,
    pub capabilities: Option<AdapterCapabilities>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterfaceType {
    Wifi,
    Ethernet,
    UsbTethering,
    Modem,
    Vpn,
    Bridge,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterfaceState {
    Up,
    Down,
    Unavailable,
    Disconnected,
}
```

### 6.3 Connection History

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionRecord {
    pub id: i64,
    pub hotspot_uuid: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub interface: String,
    pub stations_connected: u32,
    pub total_rx_bytes: u64,
    pub total_tx_bytes: u64,
}
```

### 6.4 Station (Connected Device)

```rust
#[derive(Debug, Clone)]
pub struct Station {
    pub mac: MacAddress,
    pub ip: Option<Ipv4Addr>,
    pub hostname: Option<String>,
    pub manufacturer: Option<String>,
    pub signal_dbm: i32,
    pub signal_percent: u8,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_rate_mbps: f64,
    pub tx_rate_mbps: f64,
    pub connected_since: DateTime<Utc>,
    pub is_authorized: bool,
}
```

---

## 7. Key Interfaces

### 7.1 Model -> Backend Communication

```rust
// Model sends commands to backend via async channels
pub enum BackendCommand {
    StartHotspot { config: HotspotConfig, interface: String },
    StopHotspot,
    UpdateHotspot { config: HotspotConfig },
    GetStations,
    GetAdapterInfo { interface: String },
    ScanNetworks,
    AddToBlacklist { mac: MacAddress },
    RemoveFromBlacklist { mac: MacAddress },
}

// Backend sends events back to Model
pub enum BackendEvent {
    HotspotStarted(HotspotInfo),
    HotspotStopped,
    HotspotError(String),
    StationsUpdated(Vec<Station>),
    AdapterInfo(AdapterCapabilities),
    NetworksScanned(Vec<NetworkInfo>),
    BandwidthUpdate(BandwidthSample),
}
```

### 7.2 Model -> Presenter Communication

```rust
// Events emitted by Model, consumed by Presenter on GLib main loop
pub enum UiEvent {
    HotspotStateChanged(HotspotState),
    StationsChanged(Vec<Station>),
    StatsUpdated(DashboardStats),
    ErrorOccurred(String),
    ShowToast { message: String, kind: ToastKind },
    NavigateTo(Page),
}
```

### 7.3 Firewall Interface

```rust
#[async_trait]
pub trait FirewallManager: Send + Sync {
    async fn setup_nat(&self, ap_iface: &str, upstream_iface: &str, subnet: &str) -> Result<()>;
    async fn remove_nat(&self, ap_iface: &str, upstream_iface: &str, subnet: &str) -> Result<()>;
    async fn enable_client_isolation(&self, ap_iface: &str) -> Result<()>;
    async fn add_blacklist_rule(&self, ap_iface: &str, mac: MacAddress) -> Result<()>;
    async fn remove_blacklist_rule(&self, ap_iface: &str, mac: MacAddress) -> Result<()>;
    async fn add_whitelist_rule(&self, ap_iface: &str, mac: MacAddress) -> Result<()>;
    async fn cleanup_all(&self) -> Result<()>;
}
```

---

## 8. Error Handling

```rust
#[derive(Debug, thiserror::Error)]
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

    #[error("D-Bus error: {0}")]
    DbusError(#[from] zbus::Error),

    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
}
```

---

## 9. Risk Analysis

| Risk | Severity | Mitigation |
|------|----------|------------|
| NM versions vary across distros (1.18-1.44+) | High | Test against NM 1.18 (Ubuntu 20.04), 1.30 (Fedora 35), 1.42+ (Arch). Use feature flags for API differences |
| brcmfmac driver crashes with virtual interfaces | Medium | Detect driver, warn user, disable virtual interface mode for affected hardware |
| Intel LAR prevents 5GHz AP on some adapters | Medium | Detect Intel + 5GHz, explain limitation, suggest 2.4GHz fallback |
| `iw` output format varies between versions | Medium | Parse robustly, test across iw 5.x-6.x, fallback to D-Bus capabilities |
| hostapd may be needed for advanced features | Low | Default to NM-managed AP. Only spawn hostapd for features NM cannot handle (captive portal, RADIUS) |
| dnsmasq-base vs dnsmasq package differences | Low | NM uses dnsmasq-base internally. Only install dnsmasq if custom DHCP config needed |
| SELinux/AppArmor may block operations | Medium | Run as system service or use polkit for privilege escalation. Ship AppArmor profile |
| Flatpak sandboxing limits D-Bus access | High | Use `--socket=system-bus` and `--talk-name=org.freedesktop.NetworkManager` in Flatpak manifest |
| GTK4 version differences across distros | Low | Target GTK4 4.12+ (available on all target distros). Use version checks for newer APIs |

---

## 10. Development Roadmap

### Phase 1: Foundation (Weeks 1-3)
- [x] Project structure & Cargo workspace
- [ ] `nimbus-core`: types, error handling, runtime
- [ ] `nimbus-network`: basic NM D-Bus connection via nmrs
- [ ] `nimbus-wifi`: capability detection via `iw list`
- [ ] Unit tests for core types and iw parsing

### Phase 2: Hotspot Lifecycle (Weeks 4-6)
- [ ] Create hotspot via nmrs (`AddAndActivateConnection`)
- [ ] Stop hotspot via nmrs (`DeactivateConnection`)
- [ ] Auto-detect upstream interface (Ethernet, WiFi, USB)
- [ ] NAT setup verification
- [ ] Integration tests with mock NM

### Phase 3: UI Shell (Weeks 7-9)
- [ ] GTK4/Libadwaita application skeleton
- [ ] Navigation structure (AdwNavigationView)
- [ ] Dark theme CSS
- [ ] Hotspot creation form page
- [ ] Basic status display

### Phase 4: Monitoring (Weeks 10-12)
- [ ] Connected station detection (`iw station dump`)
- [ ] Real-time bandwidth tracking
- [ ] Dashboard with live stats
- [ ] MAC OUI manufacturer lookup
- [ ] Connection history (SQLite)

### Phase 5: Advanced Features (Weeks 13-16)
- [ ] QR code generation
- [ ] Blacklist/whitelist
- [ ] Client isolation toggle
- [ ] Manual channel selection
- [ ] 5GHz band selection
- [ ] WPA3 support

### Phase 6: Polish (Weeks 17-20)
- [ ] First-run assistant
- [ ] Error recovery ("Repair" button)
- [ ] Auto-start on login
- [ ] Settings persistence (GSettings)
- [ ] Flatpak packaging
- [ ] AppStream metadata

### Phase 7: Enterprise & Extras (Weeks 21-24)
- [ ] Multiple hotspot profiles
- [ ] CLI interface
- [ ] D-Bus API for external tools
- [ ] Captive portal (optional)
- [ ] Plugin system foundation

---

## 11. Dependencies

```toml
[workspace]
members = [
    "nimbus-core",
    "nimbus-network",
    "nimbus-wifi",
    "nimbus-settings",
    "nimbus-telemetry",
    "nimbus-backend",
    "nimbus-ui",
]

[workspace.dependencies]
# Async
tokio = { version = "1", features = ["rt-multi-thread", "macros", "time", "fs", "process"] }
async-channel = "2"
async-trait = "0.1"

# GTK4 / Libadwaita
gtk = { version = "0.11", package = "gtk4", features = ["v4_14"] }
adw = { version = "0.8", package = "libadwaita", features = ["v1_7"] }
glib = "0.22"
gio = "0.22"

# NetworkManager
nmrs = "3.4"

# Serialization
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

# Error handling
thiserror = "2"
anyhow = "1"

# Logging
log = "0.4"
env_logger = "0.11"

# QR Code
qrcode = "0.14"

# Database (for history)
rusqlite = { version = "0.31", features = ["bundled"] }

# Time
chrono = { version = "0.4", features = ["serde"] }

# Testing
mockall = "0.13"
```

---

## 12. Flatpak Manifest

```json
{
  "app-id": "com.nimbus.Hotspot",
  "runtime": "org.gnome.Platform",
  "runtime-version": "47",
  "sdk": "org.gnome.Sdk",
  "sdk-extensions": ["org.freedesktop.Sdk.Extension.rust-stable"],
  "command": "nimbus-hotspot",
  "finish-args": [
    "--socket=wayland",
    "--socket=fallback-x11",
    "--socket=system-bus",
    "--socket=session-bus",
    "--talk-name=org.freedesktop.NetworkManager",
    "--talk-name=org.freedesktop.DBus",
    "--talk-name=org.freedesktop.login1",
    "--system-talk-name=org.freedesktop.NetworkManager",
    "--filesystem=host-os",
    "--ipc=host",
    "--device=all"
  ],
  "build-options": {
    "append-path": "/usr/lib/sdk/rust-stable/bin",
    "env": {
      "CARGO_HOME": "/run/build/nimbus-hotspot/cargo"
    }
  },
  "modules": [
    {
      "name": "nimbus-hotspot",
      "buildsystem": "meson",
      "sources": [
        { "type": "dir", "path": "." }
      ]
    }
  ]
}
```
