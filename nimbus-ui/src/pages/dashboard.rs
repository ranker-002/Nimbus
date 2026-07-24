use adw::prelude::*;

use nimbus_core::types::DashboardStats;

pub struct DashboardPage {
    main_box: gtk::Box,
    status_label: gtk::Label,
    stations_label: gtk::Label,
    upload_label: gtk::Label,
    download_label: gtk::Label,
    uptime_label: gtk::Label,
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

        let status_card = create_card("Status");
        let status_label = gtk::Label::new(Some("Inactive"));
        status_label.add_css_class("heading");
        status_card.append(&status_label);
        main_box.append(&status_card);

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

        Self {
            main_box,
            status_label,
            stations_label,
            upload_label,
            download_label,
            uptime_label,
        }
    }

    pub fn widget(&self) -> &gtk::Widget {
        self.main_box.upcast_ref()
    }

    pub fn update_stats(&self, stats: &DashboardStats) {
        self.stations_label
            .set_text(&stats.connected_stations.to_string());

        self.upload_label
            .set_text(&format_rate(stats.bandwidth.tx_rate));
        self.download_label
            .set_text(&format_rate(stats.bandwidth.rx_rate));

        let hours = stats.uptime_secs / 3600;
        let mins = (stats.uptime_secs % 3600) / 60;
        let secs = stats.uptime_secs % 60;
        self.uptime_label
            .set_text(&format!("{:02}:{:02}:{:02}", hours, mins, secs));
    }

    pub fn set_status(&self, status: &str) {
        self.status_label.set_text(status);
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
