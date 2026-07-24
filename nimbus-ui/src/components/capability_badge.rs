use adw::prelude::*;

use nimbus_core::types::AdapterCapabilities;

pub struct CapabilityBadge {
    box_widget: gtk::Box,
}

impl Default for CapabilityBadge {
    fn default() -> Self {
        Self::new()
    }
}

impl CapabilityBadge {
    pub fn new() -> Self {
        let box_widget = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        Self { box_widget }
    }

    pub fn widget(&self) -> &gtk::Widget {
        self.box_widget.upcast_ref()
    }

    pub fn update(&self, caps: &AdapterCapabilities) {
        while let Some(child) = self.box_widget.first_child() {
            self.box_widget.remove(&child);
        }

        add_badge(&self.box_widget, "AP", caps.supports_ap);
        add_badge(&self.box_widget, "WPA3", caps.supports_wpa3);
        add_badge(&self.box_widget, "WiFi 6", caps.supports_wifi_6);
        add_badge(&self.box_widget, "WiFi 6E", caps.supports_wifi_6e);
        add_badge(&self.box_widget, "WiFi 7", caps.supports_wifi_7);
        add_badge(
            &self.box_widget,
            "STA+AP",
            caps.supports_simultaneous_sta_ap,
        );
    }
}

fn add_badge(parent: &gtk::Box, label: &str, supported: bool) {
    let badge = gtk::Label::new(Some(label));
    if supported {
        badge.add_css_class("success");
    } else {
        badge.add_css_class("dim-label");
    }
    badge.add_css_class("caption");
    parent.append(&badge);
}
