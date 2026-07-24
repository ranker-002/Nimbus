use adw::prelude::*;
use gtk::glib;

use nimbus_core::types::StationInfo;
use nimbus_core::constants::DEFAULT_COUNTRY;

pub fn create_station_row_widget(station: &StationInfo) -> adw::ActionRow {
    let mac_str = format!("{}", station.mac);
    let short_mac = if mac_str.len() > 8 {
        &mac_str[mac_str.len() - 8..]
    } else {
        &mac_str
    };

    let title = station
        .manufacturer
        .clone()
        .unwrap_or_else(|| format!("Device {}", short_mac));

    let subtitle = format!(
        "Signal: {} dBm ({}) · ↑ {} · ↓ {}",
        station.signal_dbm,
        station.signal_percent,
        format_bytes(station.tx_bytes),
        format_bytes(station.rx_bytes),
    );

    let row = adw::ActionRow::builder()
        .title(&title)
        .subtitle(&subtitle)
        .activatable(false)
        .build();

    let signal_icon = match station.signal_percent {
        0..=25 => "network-wireless-signal-weak-symbolic",
        26..=50 => "network-wireless-signal-ok-symbolic",
        51..=75 => "network-wireless-signal-good-symbolic",
        _ => "network-wireless-signal-excellent-symbolic",
    };

    row.add_suffix(&gtk::Image::from_icon_name(signal_icon));

    row
}

fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}
