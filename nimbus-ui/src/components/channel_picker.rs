use adw::prelude::*;
use gtk::glib;

use nimbus_core::types::Band;

pub struct ChannelPicker {
    box_widget: gtk::Box,
    band_combo: gtk::DropDown,
    channel_spin: gtk::SpinButton,
}

impl ChannelPicker {
    pub fn new() -> Self {
        let box_widget = gtk::Box::new(gtk::Orientation::Vertical, 8);

        let band_label = gtk::Label::builder()
            .label("Band")
            .xalign(0.0)
            .css_classes(["dim-label"])
            .build();
        box_widget.append(&band_label);

        let band_model = gtk::StringList::new(&["Auto", "2.4 GHz", "5 GHz"]);
        let band_combo = gtk::DropDown::builder()
            .model(&band_model)
            .active(0)
            .build();
        box_widget.append(&band_combo);

        let channel_label = gtk::Label::builder()
            .label("Channel (0 = Auto)")
            .xalign(0.0)
            .css_classes(["dim-label"])
            .build();
        box_widget.append(&channel_label);

        let adjustment = gtk::Adjustment::new(0.0, 0.0, 165.0, 1.0, 10.0, 0.0);
        let channel_spin = gtk::SpinButton::builder()
            .adjustment(&adjustment)
            .hexpand(true)
            .build();
        box_widget.append(&channel_spin);

        Self {
            box_widget,
            band_combo,
            channel_spin,
        }
    }

    pub fn widget(&self) -> &gtk::Widget {
        self.box_widget.upcast_ref()
    }

    pub fn get_band(&self) -> Band {
        match self.band_combo.active() {
            0 => Band::Auto,
            1 => Band::Band2_4Ghz,
            2 => Band::Band5Ghz,
            _ => Band::Auto,
        }
    }

    pub fn get_channel(&self) -> Option<u32> {
        let val = self.channel_spin.value() as u32;
        if val > 0 { Some(val) } else { None }
    }
}
