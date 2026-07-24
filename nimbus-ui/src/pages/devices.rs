use adw::prelude::*;

use nimbus_core::types::StationInfo;

pub struct DevicesPage {
    main_box: gtk::Box,
    list: gtk::ListBox,
    count_label: gtk::Label,
}

impl Default for DevicesPage {
    fn default() -> Self {
        Self::new()
    }
}

impl DevicesPage {
    pub fn new() -> Self {
        let main_box = gtk::Box::new(gtk::Orientation::Vertical, 16);
        main_box.set_margin_top(24);
        main_box.set_margin_bottom(24);
        main_box.set_margin_start(24);
        main_box.set_margin_end(24);

        let header_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);

        let title = gtk::Label::new(Some("Connected Devices"));
        title.add_css_class("title-1");
        header_box.append(&title);

        let count_label = gtk::Label::new(Some("(0)"));
        count_label.add_css_class("dim-label");
        header_box.append(&count_label);

        main_box.append(&header_box);

        let scrolled = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .build();

        let list = gtk::ListBox::builder()
            .css_classes(["boxed-list"])
            .build();
        scrolled.set_child(Some(&list));

        main_box.append(&scrolled);

        Self {
            main_box,
            list,
            count_label,
        }
    }

    pub fn widget(&self) -> &gtk::Widget {
        self.main_box.upcast_ref()
    }

    pub fn update_stations(&self, stations: &[StationInfo]) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }

        self.count_label
            .set_text(&format!("({})", stations.len()));

        for station in stations {
            let row = create_station_row(station);
            self.list.append(&row);
        }
    }
}

fn create_station_row(station: &StationInfo) -> adw::ActionRow {
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
