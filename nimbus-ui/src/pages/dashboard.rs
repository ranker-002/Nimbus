use adw::prelude::*;
use chrono::{DateTime, Utc};

use nimbus_core::types::{ConnectionRecord, DashboardStats};

use crate::components::status_card::StatusCard;

pub struct DashboardPage {
    main_box: gtk::Box,
    status_card: StatusCard,
    stations_label: gtk::Label,
    upload_label: gtk::Label,
    download_label: gtk::Label,
    uptime_label: gtk::Label,
    history_list: gtk::ListBox,
    history_empty: gtk::Label,
}

impl Default for DashboardPage {
    fn default() -> Self {
        Self::new()
    }
}

impl DashboardPage {
    pub fn new() -> Self {
        let main_box = gtk::Box::new(gtk::Orientation::Vertical, 16);
        main_box.set_margin_top(24);
        main_box.set_margin_bottom(24);
        main_box.set_margin_start(24);
        main_box.set_margin_end(24);

        let title = gtk::Label::new(Some("Dashboard"));
        title.add_css_class("title-1");
        main_box.append(&title);

        let status_card = StatusCard::new();
        main_box.append(status_card.widget());

        let stats_grid = gtk::Grid::builder()
            .column_spacing(16)
            .row_spacing(16)
            .build();

        let stations_card = create_card("Connected Devices");
        let stations_label = gtk::Label::new(Some("0"));
        stations_label.add_css_class("title-1");
        stations_card.append(&stations_label);
        stats_grid.attach(&stations_card, 0, 0, 1, 1);

        let upload_card = create_card("Upload");
        let upload_label = gtk::Label::new(Some("0 B/s"));
        upload_label.add_css_class("heading");
        upload_card.append(&upload_label);
        stats_grid.attach(&upload_card, 1, 0, 1, 1);

        let download_card = create_card("Download");
        let download_label = gtk::Label::new(Some("0 B/s"));
        download_label.add_css_class("heading");
        download_card.append(&download_label);
        stats_grid.attach(&download_card, 2, 0, 1, 1);

        let uptime_card = create_card("Uptime");
        let uptime_label = gtk::Label::new(Some("00:00:00"));
        uptime_label.add_css_class("heading");
        uptime_card.append(&uptime_label);
        stats_grid.attach(&uptime_card, 0, 1, 1, 1);

        main_box.append(&stats_grid);

        let history_title = gtk::Label::builder()
            .label("Recent sessions")
            .xalign(0.0)
            .css_classes(["heading"])
            .margin_top(8)
            .build();
        main_box.append(&history_title);

        let history_empty = gtk::Label::builder()
            .label("No hotspot sessions recorded yet.")
            .xalign(0.0)
            .css_classes(["dim-label"])
            .build();
        main_box.append(&history_empty);

        let history_list = gtk::ListBox::builder().css_classes(["boxed-list"]).build();
        main_box.append(&history_list);

        Self {
            main_box,
            status_card,
            stations_label,
            upload_label,
            download_label,
            uptime_label,
            history_list,
            history_empty,
        }
    }

    pub fn widget(&self) -> &gtk::Widget {
        self.main_box.upcast_ref()
    }

    pub fn update_stats(&self, stats: &DashboardStats) {
        self.stations_label
            .set_text(&stats.connected_stations.to_string());

        // On the AP interface the host *receives* what clients upload, and
        // *transmits* what they download.
        self.upload_label
            .set_text(&format_rate(stats.bandwidth.rx_rate));
        self.download_label
            .set_text(&format_rate(stats.bandwidth.tx_rate));

        self.uptime_label
            .set_text(&format_duration(stats.uptime_secs));
    }

    /// Resets the live figures. Called when the hotspot goes down so the last
    /// readings do not linger as if they were current.
    pub fn clear_stats(&self) {
        self.stations_label.set_text("0");
        self.upload_label.set_text(&format_rate(0));
        self.download_label.set_text(&format_rate(0));
        self.uptime_label.set_text("00:00:00");
    }

    pub fn set_status(&self, state: &nimbus_core::types::HotspotState) {
        self.status_card.update_state(state);
    }

    pub fn show_history(&self, records: &[ConnectionRecord]) {
        while let Some(child) = self.history_list.first_child() {
            self.history_list.remove(&child);
        }

        self.history_empty.set_visible(records.is_empty());

        for record in records {
            let row = adw::ActionRow::builder()
                .title(&record.hotspot_uuid)
                .subtitle(history_subtitle(record))
                .activatable(false)
                .build();
            row.add_prefix(&gtk::Image::from_icon_name("network-wireless-symbolic"));
            self.history_list.append(&row);
        }
    }
}

fn history_subtitle(record: &ConnectionRecord) -> String {
    let started = format_time(&record.started_at);
    let duration = record
        .ended_at
        .map(|end| format_duration((end - record.started_at).num_seconds().max(0) as u64))
        .unwrap_or_else(|| "in progress".to_string());

    format!(
        "{} · {} · ↑ {} · ↓ {}",
        started,
        duration,
        format_bytes(record.total_tx_bytes),
        format_bytes(record.total_rx_bytes),
    )
}

fn format_time(time: &DateTime<Utc>) -> String {
    time.format("%Y-%m-%d %H:%M").to_string()
}

fn format_duration(total_secs: u64) -> String {
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    if hours > 0 {
        format!("{:02}:{:02}:{:02}", hours, mins, secs)
    } else {
        format!("{:02}:{:02}", mins, secs)
    }
}

fn create_card(title: &str) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 8);
    card.add_css_class("card");
    card.set_margin_top(8);
    card.set_margin_bottom(8);
    card.set_margin_start(8);
    card.set_margin_end(8);

    let label = gtk::Label::new(Some(title));
    label.add_css_class("dim-label");
    card.append(&label);

    card
}

fn format_rate(bytes_per_sec: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;

    if bytes_per_sec >= MB {
        format!("{:.1} MB/s", bytes_per_sec as f64 / MB as f64)
    } else if bytes_per_sec >= KB {
        format!("{:.1} KB/s", bytes_per_sec as f64 / KB as f64)
    } else {
        format!("{} B/s", bytes_per_sec)
    }
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
