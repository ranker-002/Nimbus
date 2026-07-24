use adw::prelude::*;

use crate::pages::dashboard::DashboardPage;
use crate::pages::hotspot::HotspotPage;
use crate::pages::devices::DevicesPage;
use crate::pages::settings::SettingsPage;

pub struct NimbusNavigationView {
    split_view: adw::NavigationSplitView,
}

impl Default for NimbusNavigationView {
    fn default() -> Self {
        Self::new()
    }
}

impl NimbusNavigationView {
    pub fn new() -> Self {
        let split_view = adw::NavigationSplitView::builder()
            .min_sidebar_width(200.0)
            .max_sidebar_width(280.0)
            .sidebar_width_fraction(0.3)
            .build();

        let nav_view = adw::NavigationView::new();

        let sidebar = create_sidebar(&nav_view);
        split_view.set_sidebar(Some(&sidebar));

        let content_page = adw::NavigationPage::builder()
            .title("Dashboard")
            .tag("dashboard")
            .child(DashboardPage::new().widget())
            .build();
        nav_view.push(&content_page);

        let nav_page = adw::NavigationPage::builder()
            .child(&nav_view)
            .build();
        split_view.set_content(Some(&nav_page));

        Self {
            split_view,
        }
    }

    pub fn widget(&self) -> &adw::NavigationSplitView {
        &self.split_view
    }
}

fn create_sidebar(nav_view: &adw::NavigationView) -> adw::NavigationPage {
    let listbox = gtk::ListBox::builder()
        .css_classes(["navigation-sidebar"])
        .build();

    let dashboard_row = create_nav_row("Dashboard", "view-grid-symbolic", "dashboard");
    let hotspot_row = create_nav_row("Hotspot", "network-wireless-symbolic", "hotspot");
    let devices_row = create_nav_row("Devices", "computer-symbolic", "devices");
    let settings_row = create_nav_row("Settings", "emblem-system-symbolic", "settings");

    listbox.append(&dashboard_row);
    listbox.append(&hotspot_row);
    listbox.append(&devices_row);
    listbox.append(&settings_row);

    let nav_view_clone = nav_view.clone();
    listbox.connect_row_activated(move |_, row| {
        let tag = row.widget_name();
        let tag = tag.as_str();
        let page = match tag {
                "dashboard" => adw::NavigationPage::builder()
                    .title("Dashboard")
                    .tag("dashboard")
                    .child(DashboardPage::new().widget())
                    .build(),
                "hotspot" => adw::NavigationPage::builder()
                    .title("Hotspot")
                    .tag("hotspot")
                    .child(HotspotPage::new().widget())
                    .build(),
                "devices" => adw::NavigationPage::builder()
                    .title("Devices")
                    .tag("devices")
                    .child(DevicesPage::new().widget())
                    .build(),
                "settings" => adw::NavigationPage::builder()
                    .title("Settings")
                    .tag("settings")
                    .child(SettingsPage::new().widget())
                    .build(),
                _ => return,
            };
            nav_view_clone.push(&page);
    });

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&listbox);

    adw::NavigationPage::builder()
        .title("Navigation")
        .child(&content)
        .build()
}

fn create_nav_row(label: &str, icon: &str, tag: &str) -> gtk::ListBoxRow {
    let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    hbox.set_margin_top(8);
    hbox.set_margin_bottom(8);
    hbox.set_margin_start(12);
    hbox.set_margin_end(12);

    let image = gtk::Image::from_icon_name(icon);
    hbox.append(&image);

    let label_widget = gtk::Label::new(Some(label));
    hbox.append(&label_widget);

    let row = gtk::ListBoxRow::builder()
        .child(&hbox)
        .activatable(true)
        .build();
    row.set_widget_name(tag);
    row
}
