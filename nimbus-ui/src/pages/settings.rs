use adw::prelude::*;

pub struct SettingsPage {
    main_box: gtk::Box,
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

        let prefs_page = adw::PreferencesPage::new();

        let general_group = adw::PreferencesGroup::builder()
            .title("General")
            .build();

        let auto_start_row = adw::SwitchRow::builder()
            .title("Auto-start on login")
            .subtitle("Automatically start the last used hotspot on login")
            .build();
        general_group.add(&auto_start_row);

        let notifications_row = adw::SwitchRow::builder()
            .title("Notifications")
            .subtitle("Show notifications for hotspot events")
            .build();
        general_group.add(&notifications_row);

        prefs_page.add(&general_group);

        let network_group = adw::PreferencesGroup::builder()
            .title("Network")
            .build();

        let max_clients_row = adw::SpinRow::builder()
            .title("Default max clients")
            .subtitle("Maximum number of connected devices per hotspot")
            .adjustment(&gtk::Adjustment::new(10.0, 1.0, 64.0, 1.0, 10.0, 0.0))
            .build();
        network_group.add(&max_clients_row);

        let country_row = adw::EntryRow::builder()
            .title("Country Code")
            .text("US")
            .build();
        network_group.add(&country_row);

        prefs_page.add(&network_group);

        let about_group = adw::PreferencesGroup::builder()
            .title("About")
            .build();

        let version_row = adw::ActionRow::builder()
            .title("Version")
            .subtitle(env!("CARGO_PKG_VERSION"))
            .activatable(false)
            .build();
        about_group.add(&version_row);

        let repo_row = adw::ActionRow::builder()
            .title("Repository")
            .subtitle("github.com/nimbus-hotspot/nimbus")
            .activatable(true)
            .build();
        about_group.add(&repo_row);

        prefs_page.add(&about_group);

        main_box.append(&prefs_page);

        Self { main_box }
    }

    pub fn widget(&self) -> &gtk::Widget {
        self.main_box.upcast_ref()
    }
}
