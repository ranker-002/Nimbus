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
- **WPA2, WPA3 and open** networks
- **2.4 GHz / 5 GHz** band selection
- **Auto channel** selection, or pin one after a scan
- **QR code** generation for instant sharing
- **Live bandwidth** and connected-device monitoring
- **Saved profiles** for the networks you reuse
- **Session history** with per-session traffic totals

</td>
<td width="50%" valign="top">

### Experience
- **Native GNOME** look (Libadwaita)
- **Dark or system** colour scheme
- **Adaptive layout** for all screen sizes
- **Desktop notifications**
- **Kick a connected device** from the device list
- **Keyboard-first CLI**
- **D-Bus service** for desktop integration

</td>
</tr>
</table>

---

## Project status

Version 0.1.0. The GUI, CLI and D-Bus service are functional on
NetworkManager-based desktops; 185 automated tests cover the planning,
capability and lifecycle rules. Not implemented: 6 GHz, captive portal and
system tray. The shared backend needs privileges — see
[Troubleshooting](#troubleshooting).

---

## How sharing works, and privileges

Nimbus picks one of two backends per start:

| Backend | When | Privileges |
|---------|------|------------|
| **NetworkManager** | The chosen adapter is free, or the shared backend is unavailable | Polkit (normal desktop user) |
| **Shared virtual AP** | You are connected over Wi-Fi and for a second interface is used, so your own connection survives | **root** + `hostapd` + `dnsmasq` |

The shared backend keeps your Wi-Fi connection up by creating a second
interface on the same radio and routing it through the existing connection.
Because it starts `hostapd`, `dnsmasq`, `nft` and `iw reg set`, it needs root:
start it with `sudo`, or run the D-Bus service (which is installed as root by
its systemd unit). The GUI falls back to the NetworkManager backend when it is
not root and warns you before doing anything that drops your connection; when
sharing is blocked by privileges it also offers **Restart as Administrator**
(through polkit) so you do not have to leave the app.

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
| `nimbus-wifi` | Wi-Fi scanning, QR codes, channel helpers |
| `nimbus-backend` | Firewall (nftables), hostapd/dnsmasq, orchestration |
| `nimbus-settings` | Hotspot profiles and preferences (JSON persistence) |
| `nimbus-telemetry` | Bandwidth sampling, session history (SQLite), OUI lookup |
| `nimbus-cli` | CLI (`nimbus start`, `nimbus stop`, …) |
| `nimbus-dbus-api` | D-Bus service for desktop integration |
| `nimbus-ui` | GTK4 + Libadwaita native interface |

---

## Install

### Build dependencies

- Rust (stable) and Cargo
- GTK4 (>= 4.12) and libadwaita (>= 1.4) development packages
- `pkg-config` (Meson only)
- `meson` and `ninja` (Meson install only)

### From source (Cargo)

```bash
git clone https://github.com/ranker-002/Nimbus.git
cd Nimbus
cargo build --release

# GUI
sudo install -Dm755 target/release/nimbus-hotspot /usr/local/bin/nimbus-hotspot
# CLI
sudo install -Dm755 target/release/nimbus /usr/local/bin/nimbus
```

### With Meson (recommended)

Meson also installs the D-Bus service and its policy, the desktop entry, the
application icon and the systemd unit:

```bash
meson setup build
ninja -C build
sudo ninja -C build install

# Enable the privileged D-Bus backend (optional, needed for the shared AP)
sudo systemctl enable --now com.nimbus.Hotspot.service
```

### Runtime requirements

- NetworkManager 1.30+
- `iw` (adapter capabilities, scanning, client list)
- For the shared backend: `hostapd`, `dnsmasq`, `nftables`, `wireless-regdb`

---

## Basic Usage

```bash
# Start a hotspot (prompts for the password, hidden)
nimbus start --ssid "MyHotspot"

# Or pass it directly / start with a specific band
nimbus start --ssid "FastHotspot" --band 5ghz --password "secretpass"

# Leave it running in the background
nimbus start --ssid "MyHotspot" --detach

# See what would happen, without changing anything
nimbus plan --ssid "MyHotspot"

# Stop the active hotspot
nimbus stop

# Show status and connected clients
nimbus status

# List Wi-Fi adapters or connected devices
nimbus devices
nimbus stations

# Scan for nearby networks
nimbus scan

# Show a QR code for sharing (from the running hotspot or given credentials)
nimbus qr
nimbus qr --ssid "MyHotspot" --password "secretpass"
```

### CLI Reference

```
nimbus start       Start a hotspot (--detach to background it)
nimbus plan        Show what start would do, without changing anything
nimbus stop        Stop the active hotspot
nimbus status      Show hotspot status & connected clients
nimbus devices     List available Wi-Fi adapters
nimbus stations    List devices connected to the hotspot
nimbus scan        Scan for nearby networks
nimbus interfaces  List all network interfaces
nimbus qr          Display a scannable QR code
nimbus --help      Show all options
```

---

## Configuration

Nimbus stores its state in `~/.config/nimbus-hotspot/`:

- `preferences.json` — defaults, notifications, dark mode, auto-start
- `hotspots.json` — saved hotspot profiles

Session history lives in the local SQLite database
`~/.local/share/nimbus-hotspot/history.db`.

---

## Troubleshooting

| Symptom | Cause and fix |
|---------|---------------|
| `Cannot start hotspot: AP mode not supported` | `iw` is missing or the adapter really has no AP mode. Install `iw`; Nimbus then checks the radio and reports precisely what is wrong. |
| `Sharing your Wi-Fi connection needs administrator rights` | The shared backend needs root. Restart Nimbus with `sudo`, enable the D-Bus service, or use the **Restart as Administrator** button in the dialog. |
| Hotspot starts but clients have no internet | Check that the adapter is not the only uplink, that `dnsmasq` is installed, and that the channel is outside DFS (52–64, 100–144) when no country is set. |
| 5 GHz refuses to start | The world regulatory domain (`00`) forbids transmitting there. Install `wireless-regdb`, set a country code, or use 2.4 GHz. |
| Hotspot started outside Nimbus is not shown | Nimbus only adopts access points it created (NetworkManager profiles named `Nimbus-*`). Stop the foreign AP first. |
| GUI shows an empty device list | `iw dev <iface> station dump` may need privileges. Run Nimbus as root or through the D-Bus service. |

---

## Supported Environments

| Category | Supported |
|----------|-----------|
| **Desktop** | GNOME, KDE, XFCE, Cinnamon, Hyprland |
| **Distro** | Arch, Fedora, Ubuntu, Debian, openSUSE |
| **Display** | Wayland, X11 |
| **NM version** | 1.30+ |
| **Package** | Cargo, Meson/Ninja |

Nimbus talks to NetworkManager over D-Bus; systems without NetworkManager
(systemd-networkd-only installations) are not supported.

---

## Development

```bash
# Clone & build
git clone https://github.com/ranker-002/Nimbus.git
cd Nimbus
cargo check --workspace
cargo clippy --workspace
cargo fmt --all

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
├── nimbus-wifi/       # Wi-Fi scanning & QR
├── nimbus-backend/    # Firewall, hostapd/dnsmasq, orchestration
├── nimbus-settings/   # Configuration persistence
├── nimbus-telemetry/  # Bandwidth, history, OUI lookup
├── nimbus-cli/        # Command-line interface
├── nimbus-dbus-api/   # D-Bus service
├── nimbus-ui/         # GTK4/Libadwaita GUI
└── data/              # Desktop files, icons, D-Bus policy
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

<a href="https://github.com/ranker-002/Nimbus/stargazers">
  <img src="https://img.shields.io/github/stars/ranker-002/Nimbus?style=social" alt="GitHub Stars"/>
</a>

</div>
