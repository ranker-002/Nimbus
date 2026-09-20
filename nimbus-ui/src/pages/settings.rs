use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;

use nimbus_core::types::{Band, Security};
use nimbus_settings::preferences::{load_preferences, save_preferences, AppPreferences};

type Changed = Box<dyn Fn(&AppPreferences)>;

pub struct SettingsPage {
    main_box: gtk::Box,
    changed: Rc<RefCell<Option<Changed>>>,
}

impl Default for SettingsPage {
    fn default() -> Self {
        Self::new()
    }
}

impl SettingsPage {
    pub fn new() -> Self {
        let main_box = gtk::Box::new(gtk::Orientation::Vertical, 16);
        main_box.set_margin_top(24);
        main_box.set_margin_bottom(24);
        main_box.set_margin_start(24);
        main_box.set_margin_end(24);

        let title = gtk::Label::new(Some("Settings"));
        title.add_css_class("title-1");
        main_box.append(&title);

        // Preferences are the source of truth for the controls below, and every
        // change is written straight back out.
        let prefs = Rc::new(RefCell::new(load_preferences()));
        let changed: Rc<RefCell<Option<Changed>>> = Rc::new(RefCell::new(None));

        let prefs_page = adw::PreferencesPage::new();

        let general_group = adw::PreferencesGroup::builder().title("General").build();

        let auto_start_row = adw::SwitchRow::builder()
            .title("Auto-start on login")
            .subtitle("Automatically start the last used hotspot on login")
            .active(prefs.borrow().auto_start)
            .build();
        general_group.add(&auto_start_row);

        let notifications_row = adw::SwitchRow::builder()
            .title("Notifications")
            .subtitle("Show notifications for hotspot events")
            .active(prefs.borrow().show_notifications)
            .build();
        general_group.add(&notifications_row);

        prefs_page.add(&general_group);

        let appearance_group = adw::PreferencesGroup::builder().title("Appearance").build();

        let dark_mode_row = adw::SwitchRow::builder()
            .title("Force dark mode")
            .subtitle("Ignore the system colour scheme and always use dark mode")
            .active(prefs.borrow().dark_mode_only)
            .build();
        appearance_group.add(&dark_mode_row);
        prefs_page.add(&appearance_group);

        let network_group = adw::PreferencesGroup::builder().title("Network").build();

        let max_clients_row = adw::SpinRow::builder()
            .title("Default max devices")
            .subtitle("Devices beyond this are disconnected. 0 means no limit.")
            .adjustment(&gtk::Adjustment::new(
                prefs.borrow().default_max_clients as f64,
                0.0,
                nimbus_core::constants::MAX_CLIENTS_MAX as f64,
                1.0,
                10.0,
                0.0,
            ))
            .build();
        network_group.add(&max_clients_row);

        let band_row = adw::ComboRow::builder()
            .title("Default band")
            .subtitle("Preselected band for a new hotspot")
            .model(&gtk::StringList::new(&["Auto", "2.4 GHz", "5 GHz"]))
            .selected(band_index(&prefs.borrow().default_band))
            .build();
        network_group.add(&band_row);

        let security_row = adw::ComboRow::builder()
            .title("Default security")
            .subtitle("Preselected security for a new hotspot")
            .model(&gtk::StringList::new(&[
                "WPA2/WPA3",
                "WPA2",
                "WPA3",
                "Open",
            ]))
            .selected(security_index(&prefs.borrow().default_security))
            .build();
        network_group.add(&security_row);

        let country_row = adw::EntryRow::builder()
            .title("Default country code")
            .text(prefs.borrow().default_country.clone().unwrap_or_default())
            .build();
        country_row.set_tooltip_text(Some(
            "Two-letter regulatory domain (e.g. FR). This applies to every \
             Wi-Fi adapter on the machine while a hotspot runs, and is put back \
             afterwards. Leave blank to keep the system setting.",
        ));
        network_group.add(&country_row);

        prefs_page.add(&network_group);

        let about_group = adw::PreferencesGroup::builder().title("About").build();

        let version_row = adw::ActionRow::builder()
            .title("Version")
            .subtitle(env!("CARGO_PKG_VERSION"))
            .activatable(false)
            .build();
        about_group.add(&version_row);

        let repo_row = adw::ActionRow::builder()
            .title("Repository")
            .subtitle("github.com/ranker-002/Nimbus")
            .activatable(true)
            .build();
        repo_row.add_suffix(&gtk::Image::from_icon_name("adw-external-link-symbolic"));
        repo_row.connect_activated(|_| {
            let launcher = gtk::UriLauncher::new("https://github.com/ranker-002/Nimbus");
            launcher.launch(None::<&gtk::Window>, None::<&gtk::gio::Cancellable>, |_| {});
        });
        about_group.add(&repo_row);

        prefs_page.add(&about_group);

        main_box.append(&prefs_page);

        let page = Self { main_box, changed };

        page.connect_switch(&auto_start_row, &prefs, move |p, value| {
            p.auto_start = value
        });
        page.connect_switch(&notifications_row, &prefs, move |p, value| {
            p.show_notifications = value
        });
        page.connect_switch(&dark_mode_row, &prefs, move |p, value| {
            p.dark_mode_only = value
        });
        page.connect_spin(&max_clients_row, &prefs, move |p, value| {
            p.default_max_clients = value
        });
        page.connect_combo(&band_row, &prefs, move |p, index| {
            p.default_band = match index {
                1 => Band::Band2_4Ghz,
                2 => Band::Band5Ghz,
                _ => Band::Auto,
            }
        });
        page.connect_combo(&security_row, &prefs, move |p, index| {
            p.default_security = match index {
                1 => Security::Wpa2,
                2 => Security::Wpa3,
                3 => Security::Open,
                _ => Security::Wpa2Wpa3Transition,
            }
        });
        page.connect_country(&country_row, &prefs);
        page.connect_auto_start(&auto_start_row);

        page
    }

    pub fn widget(&self) -> &gtk::Widget {
        self.main_box.upcast_ref()
    }

    /// Called after every persisted change, with the new preferences.
    pub fn connect_changed<F: Fn(&AppPreferences) + 'static>(&self, f: F) {
        *self.changed.borrow_mut() = Some(Box::new(f));
    }

    fn connect_switch(
        &self,
        row: &adw::SwitchRow,
        prefs: &Rc<RefCell<AppPreferences>>,
        apply: impl Fn(&mut AppPreferences, bool) + 'static,
    ) {
        let prefs = Rc::clone(prefs);
        let changed = Rc::clone(&self.changed);
        row.connect_active_notify(move |row| {
            let value = row.is_active();
            apply(&mut prefs.borrow_mut(), value);
            let snapshot = prefs.borrow().clone();
            persist(&snapshot);
            notify(&changed, &snapshot);
        });
    }

    fn connect_spin(
        &self,
        row: &adw::SpinRow,
        prefs: &Rc<RefCell<AppPreferences>>,
        apply: impl Fn(&mut AppPreferences, u32) + 'static,
    ) {
        let prefs = Rc::clone(prefs);
        let changed = Rc::clone(&self.changed);
        row.connect_value_notify(move |row| {
            let value = row.value() as u32;
            apply(&mut prefs.borrow_mut(), value);
            let snapshot = prefs.borrow().clone();
            persist(&snapshot);
            notify(&changed, &snapshot);
        });
    }

    fn connect_combo(
        &self,
        row: &adw::ComboRow,
        prefs: &Rc<RefCell<AppPreferences>>,
        apply: impl Fn(&mut AppPreferences, u32) + 'static,
    ) {
        let prefs = Rc::clone(prefs);
        let changed = Rc::clone(&self.changed);
        row.connect_selected_notify(move |row| {
            apply(&mut prefs.borrow_mut(), row.selected());
            let snapshot = prefs.borrow().clone();
            persist(&snapshot);
            notify(&changed, &snapshot);
        });
    }

    /// Saves the country only once it is a usable code, so a half-typed entry
    /// does not get written out or flagged as an error mid-keystroke.
    fn connect_country(&self, row: &adw::EntryRow, prefs: &Rc<RefCell<AppPreferences>>) {
        let prefs = Rc::clone(prefs);
        let changed = Rc::clone(&self.changed);
        row.connect_changed(move |row| {
            let text = row.text();
            let trimmed = text.trim();

            let value = if trimmed.is_empty() {
                Some(None)
            } else if nimbus_network::regdomain::is_valid(trimmed) {
                Some(Some(nimbus_network::regdomain::normalize(trimmed)))
            } else {
                None
            };

            row.set_css_classes(if value.is_some() { &[] } else { &["error"] });

            if let Some(value) = value {
                prefs.borrow_mut().default_country = value;
                let snapshot = prefs.borrow().clone();
                persist(&snapshot);
                notify(&changed, &snapshot);
            }
        });
    }

    /// Keeps the login autostart entry in step with the preference. The
    /// application also checks the preference itself, so a missing file never
    /// means a hotspot is started by surprise.
    fn connect_auto_start(&self, row: &adw::SwitchRow) {
        row.connect_active_notify(|row| {
            if let Err(e) = set_login_autostart(row.is_active()) {
                log::warn!("Could not update the autostart entry: {}", e);
            }
        });
    }
}

fn notify(changed: &Rc<RefCell<Option<Changed>>>, prefs: &AppPreferences) {
    if let Some(changed) = changed.borrow().as_ref() {
        changed(prefs);
    }
}

fn band_index(band: &Band) -> u32 {
    match band {
        Band::Auto => 0,
        Band::Band2_4Ghz => 1,
        Band::Band5Ghz => 2,
    }
}

fn security_index(security: &Security) -> u32 {
    match security {
        Security::Wpa2Wpa3Transition => 0,
        Security::Wpa2 => 1,
        Security::Wpa3 => 2,
        Security::Open => 3,
    }
}

/// Creates or removes `~/.config/autostart/com.nimbus.Hotspot.desktop`.
fn set_login_autostart(enabled: bool) -> std::io::Result<()> {
    let home = std::env::var("HOME").unwrap_or_default();
    let dir = std::path::PathBuf::from(home).join(".config/autostart");
    let path = dir.join("com.nimbus.Hotspot.desktop");

    if enabled {
        std::fs::create_dir_all(&dir)?;
        // Prefer the absolute path this build is running from, so autostart
        // works even when the binary is not on PATH.
        let exec = std::env::current_exe()
            .ok()
            .and_then(|path| path.to_str().map(str::to_string))
            .unwrap_or_else(|| "nimbus-hotspot".to_string());
        std::fs::write(
            &path,
            format!(
                "[Desktop Entry]\n\
                 Type=Application\n\
                 Name=Nimbus Hotspot\n\
                 Comment=Start the last used hotspot on login\n\
                 Exec=\"{}\" --autostart\n\
                 Icon=com.nimbus.Hotspot\n\
                 Terminal=false\n\
                 X-GNOME-Autostart-enabled=true\n",
                exec.replace('"', "\\\"")
            ),
        )?;
    } else if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

fn persist(prefs: &AppPreferences) {
    if let Err(e) = save_preferences(prefs) {
        log::error!("Failed to save preferences: {}", e);
    }
}
