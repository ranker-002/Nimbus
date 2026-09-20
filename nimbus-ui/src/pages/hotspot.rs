use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;

use nimbus_core::events::ToastKind;
use nimbus_core::types::{
    AdapterCapabilities, Band, HotspotConfig, HotspotState, InterfaceType, NetworkInterface,
    Security,
};
use nimbus_settings::hotspots::{self, SavedHotspot};

use crate::components::capability_badge::CapabilityBadge;

/// First entry of the adapter dropdown: let the backend choose.
const AUTO_ADAPTER: &str = "Automatic";
/// First entry of the profile dropdown: no profile selected.
const NO_PROFILE: &str = "Saved hotspots…";

type Notice = Box<dyn Fn(&str, ToastKind)>;
type InterfaceSelected = Box<dyn Fn(Option<String>)>;

pub struct HotspotPage {
    main_box: gtk::Box,
    ssid_entry: gtk::Entry,
    password_entry: gtk::PasswordEntry,
    adapter_combo: gtk::DropDown,
    /// Interface names in dropdown order, `None` for the "Automatic" entry.
    adapters: RefCell<Vec<Option<String>>>,
    capability_badge: CapabilityBadge,
    band_combo: gtk::DropDown,
    security_combo: gtk::DropDown,
    channel_spin: gtk::SpinButton,
    max_clients_spin: gtk::SpinButton,
    country_entry: gtk::Entry,
    hidden_switch: gtk::Switch,
    isolation_switch: gtk::Switch,
    /// Indexed like the dropdown model; index 0 means "no profile".
    profiles: RefCell<Vec<Option<SavedHotspot>>>,
    profile_combo: gtk::DropDown,
    /// Set while the profile dropdown is updated from code, so the "Loaded"
    /// notice only appears for real user selections.
    suppress_profile_notice: Cell<bool>,
    validation_label: gtk::Label,
    start_button: gtk::Button,
    stop_button: gtk::Button,
    qr_button: gtk::Button,
    scan_button: gtk::Button,
    generate_button: gtk::Button,
    save_profile_button: gtk::Button,
    delete_profile_button: gtk::Button,
    status_label: gtk::Label,
    notice: RefCell<Option<Notice>>,
    interface_selected: RefCell<Option<InterfaceSelected>>,
}

impl Default for HotspotPage {
    fn default() -> Self {
        Self::new()
    }
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

        // Saved profiles sit at the top: picking one fills the whole form.
        let profile_row = create_combo_row("Saved hotspot", &[NO_PROFILE]);
        let profile_combo = profile_row.1;
        form.append(&profile_row.0);

        let profile_buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        profile_buttons.set_halign(gtk::Align::Start);
        let save_profile_button = gtk::Button::builder()
            .label("Save as profile")
            .css_classes(["flat"])
            .build();
        let delete_profile_button = gtk::Button::builder()
            .label("Delete profile")
            .css_classes(["flat", "destructive-action"])
            .build();
        profile_buttons.append(&save_profile_button);
        profile_buttons.append(&delete_profile_button);
        form.append(&profile_buttons);

        let ssid_row = create_entry_row("Network Name (SSID)");
        let ssid_entry = ssid_row.1;
        form.append(&ssid_row.0);

        let password_row = create_password_row("Password");
        let password_entry = password_row.1;
        form.append(&password_row.0);

        let password_buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        password_buttons.set_halign(gtk::Align::Start);
        let generate_button = gtk::Button::builder()
            .label("Generate password")
            .css_classes(["flat"])
            .build();
        password_buttons.append(&generate_button);
        form.append(&password_buttons);

        let adapter_row = create_combo_row("Wi-Fi Adapter", &[AUTO_ADAPTER]);
        let adapter_combo = adapter_row.1;
        form.append(&adapter_row.0);

        let capability_badge = CapabilityBadge::new();
        form.append(capability_badge.widget());

        let band_row = create_combo_row("Band", &["Auto", "2.4 GHz", "5 GHz"]);
        let band_combo = band_row.1;
        form.append(&band_row.0);

        let security_row = create_combo_row("Security", &["WPA2/WPA3", "WPA2", "WPA3", "Open"]);
        let security_combo = security_row.1;
        form.append(&security_row.0);

        let channel_row = create_spin_row("Channel (0 = Auto)", 0.0, 165.0);
        let channel_spin = channel_row.1;
        form.append(&channel_row.0);

        let max_clients_row = create_spin_row(
            "Max Devices (0 = unlimited)",
            0.0,
            nimbus_core::constants::MAX_CLIENTS_MAX as f64,
        );
        let max_clients_spin = max_clients_row.1;
        form.append(&max_clients_row.0);

        let country_row = create_entry_row("Country Code (blank = leave system setting)");
        let country_entry = country_row.1;
        country_entry.set_max_length(2);
        form.append(&country_row.0);

        let hidden_row = create_switch_row("Hidden Network");
        let hidden_switch = hidden_row.1;
        form.append(&hidden_row.0);

        let isolation_row = create_switch_row("Client Isolation");
        let isolation_switch = isolation_row.1;
        form.append(&isolation_row.0);

        main_box.append(&form);

        let validation_label = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .css_classes(["error"])
            .visible(false)
            .build();
        main_box.append(&validation_label);

        let button_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        button_box.set_halign(gtk::Align::End);

        let scan_button = gtk::Button::builder()
            .label("Scan Networks")
            .css_classes(["flat"])
            .build();
        button_box.append(&scan_button);

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

        let status_label = gtk::Label::builder()
            .label("Hotspot inactive")
            .xalign(0.0)
            .css_classes(["dim-label"])
            .wrap(true)
            .build();
        main_box.append(&status_label);

        stop_button.set_sensitive(false);

        Self {
            main_box,
            ssid_entry,
            password_entry,
            adapter_combo,
            adapters: RefCell::new(vec![None]),
            capability_badge,
            band_combo,
            security_combo,
            channel_spin,
            max_clients_spin,
            country_entry,
            hidden_switch,
            isolation_switch,
            profiles: RefCell::new(vec![None]),
            profile_combo,
            suppress_profile_notice: Cell::new(false),
            validation_label,
            start_button,
            stop_button,
            qr_button,
            scan_button,
            generate_button,
            save_profile_button,
            delete_profile_button,
            status_label,
            notice: RefCell::new(None),
            interface_selected: RefCell::new(None),
        }
    }

    /// Connects the signals that need the page itself (profiles, validation,
    /// channel ranges). Must be called once, after the page is in an `Rc`.
    pub fn wire(self: &Rc<Self>) {
        {
            let page = Rc::clone(self);
            self.band_combo.connect_selected_notify(move |_| {
                page.sync_channel_range();
            });
        }
        {
            let page = Rc::clone(self);
            self.security_combo.connect_selected_notify(move |_| {
                page.update_validation_hint();
            });
        }
        {
            let page = Rc::clone(self);
            self.ssid_entry.connect_changed(move |_| {
                page.update_validation_hint();
            });
        }
        {
            let page = Rc::clone(self);
            self.password_entry.connect_changed(move |_| {
                page.update_validation_hint();
            });
        }
        {
            let page = Rc::clone(self);
            self.generate_button.connect_clicked(move |_| {
                page.generate_password();
            });
        }
        {
            let page = Rc::clone(self);
            self.save_profile_button.connect_clicked(move |_| {
                page.save_profile();
            });
        }
        {
            let page = Rc::clone(self);
            self.delete_profile_button.connect_clicked(move |_| {
                page.delete_profile();
            });
        }
        {
            let page = Rc::clone(self);
            self.profile_combo.connect_selected_notify(move |_| {
                let Some(profile) = page.selected_profile() else {
                    return;
                };
                page.set_config(&profile.config);
                // Programmatic selection (after saving or deleting) should not
                // announce a load the user did not ask for.
                if !page.suppress_profile_notice.get() {
                    page.tell(
                        &format!("Loaded profile '{}'", profile.name),
                        ToastKind::Info,
                    );
                }
            });
        }
        {
            let page = Rc::clone(self);
            self.adapter_combo.connect_selected_notify(move |_| {
                let name = page.selected_interface();
                if let Some(callback) = page.interface_selected.borrow().as_ref() {
                    callback(name);
                }
            });
        }

        self.refresh_profiles();
        self.update_validation_hint();
    }

    pub fn widget(&self) -> &gtk::Widget {
        self.main_box.upcast_ref()
    }

    /// Registers where user-facing hints should be reported (toasts).
    pub fn connect_notice<F: Fn(&str, ToastKind) + 'static>(&self, f: F) {
        *self.notice.borrow_mut() = Some(Box::new(f));
    }

    fn tell(&self, message: &str, kind: ToastKind) {
        if let Some(notice) = self.notice.borrow().as_ref() {
            notice(message, kind);
        }
    }

    pub fn get_config(&self) -> HotspotConfig {
        let ssid = self.ssid_entry.text().to_string();
        let password = self.password_entry.text().to_string();

        let band = match self.band_combo.selected() {
            0 => Band::Auto,
            1 => Band::Band2_4Ghz,
            2 => Band::Band5Ghz,
            _ => Band::Auto,
        };

        let security = match self.security_combo.selected() {
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

        // Zero means "no limit" in the form; the config expresses that as None.
        let max_clients = match self.max_clients_spin.value() as u32 {
            0 => None,
            n => Some(n),
        };

        // An empty field means "leave the machine's regulatory domain alone".
        let country_code = match self.country_entry.text().trim() {
            "" => None,
            code => Some(code.to_ascii_uppercase()),
        };

        HotspotConfig {
            ssid,
            password,
            band,
            channel,
            country_code,
            security,
            hidden: self.hidden_switch.is_active(),
            max_clients,
            client_isolation: self.isolation_switch.is_active(),
            auto_start: false,
        }
    }

    pub fn set_config(&self, config: &HotspotConfig) {
        self.ssid_entry.set_text(&config.ssid);
        self.password_entry.set_text(&config.password);
        self.band_combo.set_selected(match config.band {
            Band::Auto => 0,
            Band::Band2_4Ghz => 1,
            Band::Band5Ghz => 2,
        });
        self.security_combo.set_selected(match config.security {
            Security::Wpa2Wpa3Transition => 0,
            Security::Wpa2 => 1,
            Security::Wpa3 => 2,
            Security::Open => 3,
        });
        self.channel_spin
            .set_value(config.channel.unwrap_or(0) as f64);
        self.max_clients_spin
            .set_value(config.max_clients.unwrap_or(0) as f64);
        self.country_entry
            .set_text(config.country_code.as_deref().unwrap_or(""));
        self.hidden_switch.set_active(config.hidden);
        self.isolation_switch.set_active(config.client_isolation);
        self.sync_channel_range();
        self.update_validation_hint();
    }

    /// Seeds the form from saved preferences. Only fills fields the user has
    /// not already touched, so live settings changes do not overwrite a
    /// half-typed hotspot.
    pub fn apply_defaults(&self, config: &HotspotConfig) {
        if self.ssid_entry.text().is_empty() {
            self.ssid_entry.set_text(&config.ssid);
        }
        self.band_combo.set_selected(match config.band {
            Band::Auto => 0,
            Band::Band2_4Ghz => 1,
            Band::Band5Ghz => 2,
        });
        self.security_combo.set_selected(match config.security {
            Security::Wpa2Wpa3Transition => 0,
            Security::Wpa2 => 1,
            Security::Wpa3 => 2,
            Security::Open => 3,
        });
        self.max_clients_spin
            .set_value(config.max_clients.unwrap_or(0) as f64);
        if let Some(code) = &config.country_code {
            self.country_entry.set_text(code);
        }
        self.sync_channel_range();
    }

    /// Shows the domain the machine is currently using as placeholder text, so
    /// an empty field visibly means "keep this".
    pub fn set_regulatory_domain(&self, domain: Option<&str>) {
        let hint = match domain {
            Some("00") => "00 (world / unset)".to_string(),
            Some(code) => code.to_string(),
            None => "unknown".to_string(),
        };
        self.country_entry
            .set_placeholder_text(Some(&format!("System setting: {}", hint)));
    }

    /// Repopulates the adapter dropdown, keeping the current choice if it is
    /// still present.
    pub fn set_interfaces(&self, interfaces: &[NetworkInterface]) {
        let wifi: Vec<&NetworkInterface> = interfaces
            .iter()
            .filter(|i| i.interface_type == InterfaceType::Wifi)
            .collect();

        let previous = self.selected_interface();

        let mut labels = vec![AUTO_ADAPTER.to_string()];
        let mut names: Vec<Option<String>> = vec![None];
        for iface in &wifi {
            labels.push(format!("{} ({})", iface.name, describe_state(iface)));
            names.push(Some(iface.name.clone()));
        }

        let restored = previous
            .and_then(|name| names.iter().position(|n| n.as_deref() == Some(&*name)))
            .unwrap_or(0);

        let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        self.adapter_combo
            .set_model(Some(&gtk::StringList::new(&label_refs)));
        self.adapter_combo.set_selected(restored as u32);
        *self.adapters.borrow_mut() = names;
    }

    pub fn set_capabilities(&self, caps: &AdapterCapabilities) {
        self.capability_badge.update(caps);
    }

    /// Pins the channel field, e.g. after the user picked one from a scan.
    pub fn set_channel(&self, channel: u32) {
        if channel == 0 {
            return;
        }
        // Selecting a channel as well as a band keeps the pair consistent.
        match Band::of_channel(channel) {
            Band::Band2_4Ghz => self.band_combo.set_selected(1),
            Band::Band5Ghz => self.band_combo.set_selected(2),
            Band::Auto => {}
        }
        self.sync_channel_range();
        self.channel_spin.set_value(channel as f64);
    }

    /// The adapter the user picked, or `None` to let the backend choose.
    pub fn selected_interface(&self) -> Option<String> {
        self.adapters
            .borrow()
            .get(self.adapter_combo.selected() as usize)
            .cloned()
            .flatten()
    }

    pub fn connect_start<F: Fn() + 'static>(&self, f: F) {
        let f = Rc::new(f);
        self.start_button.connect_clicked(move |_| f());
    }

    pub fn connect_stop<F: Fn() + 'static>(&self, f: F) {
        let f = Rc::new(f);
        self.stop_button.connect_clicked(move |_| f());
    }

    pub fn connect_qr<F: Fn() + 'static>(&self, f: F) {
        let f = Rc::new(f);
        self.qr_button.connect_clicked(move |_| f());
    }

    pub fn connect_scan<F: Fn() + 'static>(&self, f: F) {
        let f = Rc::new(f);
        self.scan_button.connect_clicked(move |_| f());
    }

    /// Called whenever the adapter choice changes, with the new interface name
    /// (`None` for Automatic). Register before calling [`wire`](Self::wire).
    pub fn connect_interface_selected<F: Fn(Option<String>) + 'static>(&self, f: F) {
        *self.interface_selected.borrow_mut() = Some(Box::new(f));
    }

    /// Mirrors the backend state onto the controls, so the buttons can never
    /// offer an action the backend is not in a position to carry out.
    pub fn set_state(&self, state: &HotspotState) {
        let (can_start, can_stop, status) = match state {
            HotspotState::Inactive => (true, false, "Hotspot inactive".to_string()),
            HotspotState::Starting => (false, false, "Starting hotspot…".to_string()),
            HotspotState::Active(ssid) => (false, true, format!("Hotspot '{}' is running", ssid)),
            HotspotState::Stopping => (false, false, "Stopping hotspot…".to_string()),
            HotspotState::Error(message) => (true, false, format!("Error: {}", message)),
        };

        self.start_button.set_sensitive(can_start);
        self.stop_button.set_sensitive(can_stop);
        // Nothing about the running hotspot can be edited, and there is no
        // point changing the form while it is coming up or going down.
        for control in [
            self.ssid_entry.upcast_ref::<gtk::Widget>(),
            self.password_entry.upcast_ref(),
            self.adapter_combo.upcast_ref(),
            self.band_combo.upcast_ref(),
            self.security_combo.upcast_ref(),
            self.channel_spin.upcast_ref(),
            self.max_clients_spin.upcast_ref(),
            self.country_entry.upcast_ref(),
            self.hidden_switch.upcast_ref(),
            self.isolation_switch.upcast_ref(),
            self.profile_combo.upcast_ref(),
            self.generate_button.upcast_ref(),
            self.save_profile_button.upcast_ref(),
            self.delete_profile_button.upcast_ref(),
        ] {
            control.set_sensitive(can_start);
        }
        self.status_label.set_text(&status);
    }

    /// Regenerates the password field with a random, WPA-compatible passphrase.
    pub fn generate_password(&self) {
        let random = uuid::Uuid::new_v4().simple().to_string();
        // 16 hex characters is long enough for WPA and easy to type.
        self.password_entry.set_text(&random[..16]);
        self.update_validation_hint();
    }

    pub fn refresh_profiles(&self) {
        let saved = hotspots::load_hotspots().unwrap_or_default();
        let mut labels = vec![NO_PROFILE.to_string()];
        let mut entries: Vec<Option<SavedHotspot>> = vec![None];
        for profile in saved {
            labels.push(profile.name.clone());
            entries.push(Some(profile));
        }

        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        self.profile_combo
            .set_model(Some(&gtk::StringList::new(&refs)));
        self.profile_combo.set_selected(0);
        *self.profiles.borrow_mut() = entries;
    }

    fn selected_profile(&self) -> Option<SavedHotspot> {
        self.profiles
            .borrow()
            .get(self.profile_combo.selected() as usize)
            .cloned()
            .flatten()
    }

    fn save_profile(&self) {
        let config = self.get_config();
        if let Err(e) = config.validate() {
            self.tell(&e.to_string(), ToastKind::Error);
            return;
        }
        match hotspots::save_hotspot(&config) {
            Ok(saved) => {
                self.suppress_profile_notice.set(true);
                self.refresh_profiles();
                // Select the freshly saved profile so Delete acts on it.
                if let Some(index) = self
                    .profiles
                    .borrow()
                    .iter()
                    .position(|p| p.as_ref().is_some_and(|p| p.uuid == saved.uuid))
                {
                    self.profile_combo.set_selected(index as u32);
                }
                self.suppress_profile_notice.set(false);
                self.tell(
                    &format!("Saved profile '{}'", saved.name),
                    ToastKind::Success,
                );
            }
            Err(e) => self.tell(&format!("Could not save profile: {}", e), ToastKind::Error),
        }
    }

    fn delete_profile(&self) {
        let Some(profile) = self.selected_profile() else {
            self.tell("Select a saved profile first", ToastKind::Warning);
            return;
        };
        match hotspots::delete_hotspot(&profile.uuid) {
            Ok(()) => {
                self.refresh_profiles();
                self.tell(
                    &format!("Deleted profile '{}'", profile.name),
                    ToastKind::Info,
                );
            }
            Err(e) => self.tell(
                &format!("Could not delete profile: {}", e),
                ToastKind::Error,
            ),
        }
    }

    /// Re-checks the fields that block starting and marks them visibly.
    pub fn update_validation_hint(&self) {
        let security_open = self.security_combo.selected() == 3;
        let ssid_ok = !self.ssid_entry.text().trim().is_empty();
        let password_ok = security_open
            || self.password_entry.text().len() >= nimbus_core::constants::MIN_PASSWORD_LEN;

        set_error(&self.ssid_entry, !ssid_ok);
        set_error(&self.password_entry, !password_ok);

        let message = if !ssid_ok {
            Some("Enter a network name (SSID).")
        } else if !password_ok {
            Some("The password must be at least 8 characters.")
        } else {
            None
        };

        match message {
            Some(text) => {
                self.validation_label.set_text(text);
                self.validation_label.set_visible(true);
            }
            None => self.validation_label.set_visible(false),
        }
    }

    fn sync_channel_range(&self) {
        let (upper, step) = match self.band_combo.selected() {
            1 => (13.0, 1.0),
            2 => (165.0, 4.0),
            _ => (165.0, 1.0),
        };
        let adjustment = self.channel_spin.adjustment();
        let value = self.channel_spin.value();
        adjustment.set_upper(upper);
        adjustment.set_step_increment(step);
        adjustment.set_page_increment(step * 4.0);
        if value > upper {
            self.channel_spin.set_value(0.0);
        }
    }
}

fn set_error(widget: &impl IsA<gtk::Widget>, error: bool) {
    if error {
        widget.add_css_class("error");
    } else {
        widget.remove_css_class("error");
    }
}

fn describe_state(iface: &NetworkInterface) -> &'static str {
    match iface.state {
        nimbus_core::types::InterfaceState::Up => "connected",
        nimbus_core::types::InterfaceState::Disconnected => "available",
        nimbus_core::types::InterfaceState::Unavailable => "unavailable",
        nimbus_core::types::InterfaceState::Down => "unmanaged",
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

    let entry = gtk::Entry::builder().hexpand(true).build();
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
    let dropdown = gtk::DropDown::builder().model(&model).selected(0).build();
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

    let switch = gtk::Switch::builder().valign(gtk::Align::Center).build();
    row.append(&switch);

    (row, switch)
}
