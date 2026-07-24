use adw::prelude::*;
use gtk::glib;

use nimbus_core::types::{Band, HotspotConfig, Security};

pub struct HotspotPage {
    main_box: gtk::Box,
    ssid_entry: gtk::Entry,
    password_entry: gtk::PasswordEntry,
    band_combo: gtk::DropDown,
    security_combo: gtk::DropDown,
    channel_spin: gtk::SpinButton,
    hidden_switch: gtk::Switch,
    isolation_switch: gtk::Switch,
    start_button: gtk::Button,
    stop_button: gtk::Button,
    qr_button: gtk::Button,
}

impl HotspotPage {
    pub fn new() -> Self {
        let main_box = gtk::Box::new(gtk::Orientation::Vertical, 16);
        main_box.set_margin_top(24);
        main_box.set_margin_bottom(24);
        main_box.set_margin_start(24);
        main_box.set_margin_end(24);

        let title = gtk::Label::new(Some("Create Hotspot"));
        title.add_css_class("title-1");
        main_box.append(&title);

        let form = gtk::Box::new(gtk::Orientation::Vertical, 12);

        let ssid_row = create_entry_row("Network Name (SSID)");
        let ssid_entry = ssid_row.1;
        form.append(&ssid_row.0);

        let password_row = create_password_row("Password");
        let password_entry = password_row.1;
        form.append(&password_row.0);

        let band_row = create_combo_row("Band", &["Auto", "2.4 GHz", "5 GHz"]);
        let band_combo = band_row.1;
        form.append(&band_row.0);

        let security_row = create_combo_row("Security", &["WPA2/WPA3", "WPA2", "WPA3", "Open"]);
        let security_combo = security_row.1;
        form.append(&security_row.0);

        let channel_row = create_spin_row("Channel (0 = Auto)", 0, 165);
        let channel_spin = channel_row.1;
        form.append(&channel_row.0);

        let hidden_row = create_switch_row("Hidden Network");
        let hidden_switch = hidden_row.1;
        form.append(&hidden_row.0);

        let isolation_row = create_switch_row("Client Isolation");
        let isolation_switch = isolation_row.1;
        form.append(&isolation_row.0);

        main_box.append(&form);

        let button_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        button_box.set_halign(gtk::Align::End);

        let qr_button = gtk::Button::builder()
            .label("Show QR Code")
            .css_classes(["flat"])
            .build();
        button_box.append(&qr_button);

        let stop_button = gtk::Button::builder()
            .label("Stop Hotspot")
            .css_classes(["destructive-action"])
            .build();
        button_box.append(&stop_button);

        let start_button = gtk::Button::builder()
            .label("Start Hotspot")
            .css_classes(["suggested-action"])
            .build();
        button_box.append(&start_button);

        main_box.append(&button_box);

        Self {
            main_box,
            ssid_entry,
            password_entry,
            band_combo,
            security_combo,
            channel_spin,
            hidden_switch,
            isolation_switch,
            start_button,
            stop_button,
            qr_button,
        }
    }

    pub fn widget(&self) -> &gtk::Widget {
        self.main_box.upcast_ref()
    }

    pub fn get_config(&self) -> HotspotConfig {
        let ssid = self.ssid_entry.text().to_string();
        let password = self.password_entry.text().to_string();

        let band = match self.band_combo.active() {
            0 => Band::Auto,
            1 => Band::Band2_4Ghz,
            2 => Band::Band5Ghz,
            _ => Band::Auto,
        };

        let security = match self.security_combo.active() {
            0 => Security::Wpa2Wpa3Transition,
            1 => Security::Wpa2,
            2 => Security::Wpa3,
            3 => Security::Open,
            _ => Security::Wpa2Wpa3Transition,
        };

        let channel = if self.channel_spin.value() > 0.0 {
            Some(self.channel_spin.value() as u32)
        } else {
            None
        };

        HotspotConfig {
            ssid,
            password,
            band,
            channel,
            country_code: "US".to_string(),
            security,
            hidden: self.hidden_switch.is_active(),
            max_clients: Some(10),
            client_isolation: self.isolation_switch.is_active(),
            ipv4_method: nimbus_core::types::Ipv4Method::Shared,
            auto_start: false,
        }
    }

    pub fn connect_start<F: Fn() + 'static>(&self, f: F) {
        self.start_button.connect_clicked(move |_| f());
    }

    pub fn connect_stop<F: Fn() + 'static>(&self, f: F) {
        self.stop_button.connect_clicked(move |_| f());
    }

    pub fn connect_qr<F: Fn() + 'static>(&self, f: F) {
        self.qr_button.connect_clicked(move |_| f());
    }

    pub fn set_active(&self, active: bool) {
        self.start_button.set_sensitive(!active);
        self.stop_button.set_sensitive(active);
    }
}

fn create_entry_row(title: &str) -> (gtk::Box, gtk::Entry) {
    let row = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let label = gtk::Label::builder()
        .label(title)
        .xalign(0.0)
        .css_classes(["dim-label"])
        .build();
    row.append(&label);

    let entry = gtk::Entry::builder()
        .hexpand(true)
        .build();
    row.append(&entry);

    (row, entry)
}

fn create_password_row(title: &str) -> (gtk::Box, gtk::PasswordEntry) {
    let row = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let label = gtk::Label::builder()
        .label(title)
        .xalign(0.0)
        .css_classes(["dim-label"])
        .build();
    row.append(&label);

    let entry = gtk::PasswordEntry::builder()
        .hexpand(true)
        .show_peek_icon(true)
        .build();
    row.append(&entry);

    (row, entry)
}

fn create_combo_row(title: &str, options: &[&str]) -> (gtk::Box, gtk::DropDown) {
    let row = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let label = gtk::Label::builder()
        .label(title)
        .xalign(0.0)
        .css_classes(["dim-label"])
        .build();
    row.append(&label);

    let model = gtk::StringList::new(options);
    let dropdown = gtk::DropDown::builder()
        .model(&model)
        .active(0)
        .build();
    row.append(&dropdown);

    (row, dropdown)
}

fn create_spin_row(title: &str, min: f64, max: f64) -> (gtk::Box, gtk::SpinButton) {
    let row = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let label = gtk::Label::builder()
        .label(title)
        .xalign(0.0)
        .css_classes(["dim-label"])
        .build();
    row.append(&label);

    let adjustment = gtk::Adjustment::new(0.0, min, max, 1.0, 10.0, 0.0);
    let spin = gtk::SpinButton::builder()
        .adjustment(&adjustment)
        .hexpand(true)
        .build();
    row.append(&spin);

    (row, spin)
}

fn create_switch_row(title: &str) -> (gtk::Box, gtk::Switch) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let label = gtk::Label::builder()
        .label(title)
        .hexpand(true)
        .xalign(0.0)
        .build();
    row.append(&label);

    let switch = gtk::Switch::builder()
        .valign(gtk::Align::Center)
        .build();
    row.append(&switch);

    (row, switch)
}
