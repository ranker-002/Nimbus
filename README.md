<div align="center">

<!-- Logo SVG -->
<svg width="120" height="120" viewBox="0 0 120 120" fill="none" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <linearGradient id="nimbus-grad" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" style="stop-color:#6366f1"/>
      <stop offset="100%" style="stop-color:#8b5cf6"/>
    </linearGradient>
    <filter id="glow">
      <feGaussianBlur stdDeviation="3" result="coloredBlur"/>
      <feMerge><feMergeNode in="coloredBlur"/><feMergeNode in="SourceGraphic"/></feMerge>
    </filter>
  </defs>
  <rect width="120" height="120" rx="28" fill="url(#nimbus-grad)"/>
  <!-- Wi-Fi waves -->
  <g transform="translate(60,52)" stroke="white" stroke-width="3.5" stroke-linecap="round" fill="none" filter="url(#glow)">
    <path d="M-30,15 a42,42 0 0,1 60,0" opacity="0.35"/>
    <path d="M-22,8 a30,30 0 0,1 44,0" opacity="0.55"/>
    <path d="M-14,1 a18,18 0 0,1 28,0" opacity="0.8"/>
    <circle cx="0" cy="18" r="4" fill="white"/>
  </g>
  <!-- N letter -->
  <text x="60" y="105" text-anchor="middle" font-family="'Inter','Segoe UI',sans-serif" font-weight="800" font-size="18" fill="white" opacity="0.9">NIMBUS</text>
</svg>

<br/>

# Nimbus

**A modern Wi-Fi hotspot manager for Linux.**

[![License: MIT](https://img.shields.io/badge/License-MIT-6366f1.svg?style=flat-square)](LICENSE)
[![Rust](https://img.shields.io/badge/Built%20with-Rust-orange.svg?style=flat-square&logo=rust)](https://rust-lang.org)
[![GTK4](https://img.shields.io/badge/UI-GTK4%20%2F%20Libadwaita-blue.svg?style=flat-square&logo=gnome)](https://gtk.org)
[![Platform](https://img.shields.io/badge/Platform-Linux-lightgrey.svg?style=flat-square)](https://linux.org)

Nimbus replaces legacy tools like `create_ap` with a clean, native experience.
Create a hotspot in two clicks. Share your internet instantly.

</div>

---

## Features

<table>
<tr>
<td width="50%" valign="top">

### Core
- **One-click hotspot** from any Wi-Fi adapter
- **WPA2 & WPA3** security (auto-detected)
- **2.4 GHz / 5 GHz / 6 GHz** band selection
- **Auto channel** selection for best performance
- **QR code** generation for instant sharing
- **Per-hotspot** bandwidth tracking

</td>
<td width="50%" valign="top">

### Experience
- **Native GNOME** look (Libadwaita)
- **Dark mode** built-in
- **Adaptive layout** for all screen sizes
- **System tray** integration
- **Keyboard-first** CLI
- **Flatpak** sandboxed install

</td>
</tr>
</table>

---

## Architecture

```
┌─────────────────────────────────────────────────┐
│                  Nimbus                          │
├──────────┬──────────┬──────────┬────────────────┤
│ CLI      │ GTK4 UI  │ D-Bus    │ Backend        │
│ nimbus-  │ nimbus-  │ nimbus-  │ nimbus-        │
│ cli      │ ui       │ dbus-api │ backend        │
├──────────┴──────────┴──────────┴────────────────┤
│  nimbus-network  ·  nimbus-wifi  ·  nimbus-core  │
│  nimbus-settings ·  nimbus-telemetry             │
├─────────────────────────────────────────────────┤
│  NetworkManager (nmrs) · nftables · iw · Tokio   │
└─────────────────────────────────────────────────┘
```

| Crate | Purpose |
|-------|---------|
| `nimbus-core` | Shared types, errors, constants |
| `nimbus-network` | NetworkManager D-Bus operations via `nmrs` |
| `nimbus-wifi` | Capability detection, QR codes, channel selection |
| `nimbus-backend` | Firewall (nftables), DNS, captive portal orchestration |
| `nimbus-settings` | Hotspot configs, user preferences (JSON persistence) |
| `nimbus-telemetry` | Bandwidth monitoring, history (SQLite), MAC lookup |
| `nimbus-cli` | Keyboard-first CLI (`nimbus start`, `nimbus stop`, ...) |
| `nimbus-dbus-api` | D-Bus service for desktop integration |
| `nimbus-ui` | GTK4 + Libadwaita native interface |

---

## Tech Stack

<div align="center">

| | Technology | Role |
|---|---|---|
| <img src="https://raw.githubusercontent.com/nickvdyck/awesome-rust/main/assets/rust.svg" width="20" height="20"> | **Rust** | Memory-safe, zero-cost abstractions |
| <img src="https://raw.githubusercontent.com/nickvdyck/awesome-rust/main/assets/tokio.svg" width="20" height="20"> | **Tokio** | Async runtime for backend operations |
| <img src="https://raw.githubusercontent.com/nickvdyck/awesome-rust/main/assets/zbus.svg" width="20" height="20"> | **zbus** | Pure-Rust D-Bus implementation |
| <img src="https://raw.githubusercontent.com/nickvdyck/awesome-rust/main/assets/nmrs.svg" width="20" height="20"> | **nmrs** | NetworkManager D-Bus API bindings |
| <img src="https://raw.githubusercontent.com/nickvdyck/awesome-rust/main/assets/gtk4rs.svg" width="20" height="20"> | **GTK4-rs** | Rust bindings for GTK4 |
| | **Libadwaita** | GNOME-native adaptive UI |
| | **nftables** | Modern firewall & NAT rules |

</div>

---

## Quick Start

### Install

```bash
# From source
git clone https://github.com/ranker-002/Nimbus.git
cd Nimbus
cargo build --release
sudo cp target/release/nimbus /usr/local/bin/
```

### Basic Usage

```bash
# Start a hotspot (auto-detects adapter)
nimbus start --ssid "MyHotspot" --password "secretpass"

# Start with specific band
nimbus start --ssid "FastHotspot" --band 5ghz

# Stop the active hotspot
nimbus stop

# Show status
nimbus status

# List adapters
nimbus devices

# Scan for networks
nimbus scan

# Show QR code for sharing
nimbus qr
```

### CLI Reference

```
nimbus start      Start a new hotspot
nimbus stop       Stop the active hotspot
nimbus status     Show hotspot status & connected clients
nimbus devices    List available Wi-Fi adapters
nimbus scan       Scan for nearby networks
nimbus interfaces List all network interfaces
nimbus qr        Display QR code for current hotspot
nimbus --help     Show all options
```

---

## Configuration

Nimbus stores hotspot profiles in `~/.config/nimbus/hotspots.json`:

```json
{
  "profiles": [
    {
      "name": "Home",
      "ssid": "MyHomeWiFi",
      "password": "s3cure!",
      "band": "5ghz",
      "channel": 36,
      "security": "wpa3",
      "interface": "wlan0"
    }
  ]
}
```

---

## Supported Environments

| Category | Supported |
|----------|-----------|
| **Desktop** | GNOME, KDE, XFCE, Cinnamon, Hyprland |
| **Distro** | Arch, Fedora, Ubuntu, Debian, openSUSE |
| **Display** | Wayland, X11 |
| **NM version** | 1.30+ (nftables support) |
| **Package** | Flatpak (primary), Cargo install |

---

## Development

```bash
# Clone & build
git clone https://github.com/ranker-002/Nimbus.git
cd Nimbus
cargo check --workspace
cargo clippy --workspace

# Run tests
cargo test --workspace

# Run the CLI
cargo run --bin nimbus -- start --ssid TestHotspot --password password123
```

### Project Structure

```
Nimbus/
├── nimbus-core/       # Types, errors, constants
├── nimbus-network/    # NetworkManager API via nmrs
├── nimbus-wifi/       # Wi-Fi capabilities & QR
├── nimbus-backend/    # Firewall, DNS, captive portal
├── nimbus-settings/   # Configuration persistence
├── nimbus-telemetry/  # Bandwidth & history tracking
├── nimbus-cli/        # Command-line interface
├── nimbus-dbus-api/   # D-Bus service
├── nimbus-ui/         # GTK4/Libadwaita GUI
├── data/              # Desktop files, schemas, icons
└── tests/             # Integration tests
```

---

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

---

## License

This project is licensed under the **MIT License** — see the [LICENSE](LICENSE) file for details.

---

<div align="center">

**Built with care for the Linux desktop.**

<br/>

<!-- Footer badge row -->
<a href="https://github.com/ranker-002/Nimbus/stargazers">
  <img src="https://img.shields.io/github/stars/ranker-002/Nimbus?style=social" alt="GitHub Stars"/>
</a>

</div>
