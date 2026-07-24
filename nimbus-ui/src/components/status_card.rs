use adw::prelude::*;
use gtk::glib;

use nimbus_core::types::HotspotState;

pub struct StatusCard {
    card: gtk::Box,
    status_icon: gtk::Image,
    status_label: gtk::Label,
    details_label: gtk::Label,
}

impl StatusCard {
    pub fn new() -> Self {
        let card = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .css_classes(["card", "activatable"])
            .margin_top(8)
            .margin_bottom(8)
            .build();

        let status_icon = gtk::Image::from_icon_name("network-offline-symbolic");
        status_icon.set_pixel_size(48);
        card.append(&status_icon);

        let status_label = gtk::Label::new(Some("Inactive"));
        status_label.add_css_class("heading");
        card.append(&status_label);

        let details_label = gtk::Label::new(Some("No hotspot active"));
        details_label.add_css_class("dim-label");
        card.append(&details_label);

        Self {
            card,
            status_icon,
            status_label,
            details_label,
        }
    }

    pub fn widget(&self) -> &gtk::Widget {
        self.card.upcast_ref()
    }

    pub fn update_state(&self, state: &HotspotState) {
        match state {
            HotspotState::Inactive => {
                self.status_icon
                    .set_from_icon_name(Some("network-offline-symbolic"));
                self.status_label.set_text("Inactive");
                self.details_label.set_text("No hotspot active");
            }
            HotspotState::Starting => {
                self.status_icon
                    .set_from_icon_name(Some("network-wireless-acquiring-symbolic"));
                self.status_label.set_text("Starting...");
                self.details_label.set_text("Configuring hotspot");
            }
            HotspotState::Active(ssid) => {
                self.status_icon
                    .set_from_icon_name(Some("network-wireless-signal-excellent-symbolic"));
                self.status_label.set_text("Active");
                self.details_label
                    .set_text(&format!("Hotspot: {}", ssid));
            }
            HotspotState::Stopping => {
                self.status_icon
                    .set_from_icon_name(Some("network-wireless-disconnecting-symbolic"));
                self.status_label.set_text("Stopping...");
                self.details_label.set_text("Shutting down hotspot");
            }
            HotspotState::Error(msg) => {
                self.status_icon
                    .set_from_icon_name(Some("dialog-error-symbolic"));
                self.status_label.set_text("Error");
                self.details_label.set_text(msg);
            }
        }
    }
}
