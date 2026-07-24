use std::cell::RefCell;

use gtk::glib::subclass::prelude::*;
use gtk::glib;

use nimbus_core::types::DashboardStats;

mod imp {
    use super::*;
    use gtk::glib::Properties;
    use gtk::glib::object::ObjectExt;
    use std::cell::Cell;

    #[derive(Properties, Default)]
    #[properties(wrapper_type = super::DashboardViewModel)]
    pub struct DashboardViewModel {
        #[property(get, set)]
        pub status: RefCell<String>,
        #[property(get, set)]
        pub stations: Cell<u32>,
        #[property(get, set)]
        pub upload_rate: RefCell<String>,
        #[property(get, set)]
        pub download_rate: RefCell<String>,
        #[property(get, set)]
        pub uptime: RefCell<String>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for DashboardViewModel {
        const NAME: &'static str = "DashboardViewModel";
        type Type = super::DashboardViewModel;
    }

    #[glib::derived_properties]
    impl ObjectImpl for DashboardViewModel {}
}

glib::wrapper! {
    pub struct DashboardViewModel(ObjectSubclass<imp::DashboardViewModel>);
}

impl Default for DashboardViewModel {
    fn default() -> Self {
        Self::new()
    }
}

impl DashboardViewModel {
    pub fn new() -> Self {
        glib::Object::builder().build()
    }

    pub fn update_stats(&self, stats: &DashboardStats) {
        self.set_stations(stats.connected_stations);

        self.set_upload_rate(format_rate(stats.bandwidth.tx_rate));
        self.set_download_rate(format_rate(stats.bandwidth.rx_rate));

        let hours = stats.uptime_secs / 3600;
        let mins = (stats.uptime_secs % 3600) / 60;
        let secs = stats.uptime_secs % 60;
        self.set_uptime(format!("{:02}:{:02}:{:02}", hours, mins, secs));
    }
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
