use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use mac_address::MacAddress;

use nimbus_core::types::StationInfo;

use crate::components::station_row::create_station_row_widget;

type Kick = Box<dyn Fn(MacAddress)>;

pub struct DevicesPage {
    main_box: gtk::Box,
    list: gtk::ListBox,
    count_label: gtk::Label,
    empty_label: gtk::Label,
    /// Shared so buttons created before the callback was registered still find
    /// it when clicked.
    kick: Rc<RefCell<Option<Kick>>>,
}

impl Default for DevicesPage {
    fn default() -> Self {
        Self::new()
    }
}

impl DevicesPage {
    pub fn new() -> Self {
        let main_box = gtk::Box::new(gtk::Orientation::Vertical, 16);
        main_box.set_margin_top(24);
        main_box.set_margin_bottom(24);
        main_box.set_margin_start(24);
        main_box.set_margin_end(24);

        let header_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);

        let title = gtk::Label::new(Some("Connected Devices"));
        title.add_css_class("title-1");
        header_box.append(&title);

        let count_label = gtk::Label::new(Some("(0)"));
        count_label.add_css_class("dim-label");
        header_box.append(&count_label);

        main_box.append(&header_box);

        let empty_label = gtk::Label::builder()
            .label("No devices are connected yet.")
            .css_classes(["dim-label"])
            .margin_top(24)
            .build();
        main_box.append(&empty_label);

        let scrolled = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .build();

        let list = gtk::ListBox::builder().css_classes(["boxed-list"]).build();
        scrolled.set_child(Some(&list));

        main_box.append(&scrolled);

        Self {
            main_box,
            list,
            count_label,
            empty_label,
            kick: Rc::new(RefCell::new(None)),
        }
    }

    pub fn widget(&self) -> &gtk::Widget {
        self.main_box.upcast_ref()
    }

    /// Called when the user asks to disconnect a device.
    pub fn connect_kick<F: Fn(MacAddress) + 'static>(&self, f: F) {
        *self.kick.borrow_mut() = Some(Box::new(f));
    }

    pub fn update_stations(&self, stations: &[StationInfo]) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }

        self.count_label.set_text(&format!("({})", stations.len()));
        self.empty_label.set_visible(stations.is_empty());

        for station in stations {
            let row = create_station_row_widget(station);

            let kick_button = gtk::Button::builder()
                .icon_name("window-close-symbolic")
                .tooltip_text("Disconnect this device")
                .css_classes(["flat"])
                .valign(gtk::Align::Center)
                .build();

            let mac = station.mac;
            let kick = Rc::clone(&self.kick);
            kick_button.connect_clicked(move |_| {
                if let Some(callback) = kick.borrow().as_ref() {
                    callback(mac);
                }
            });

            row.add_suffix(&kick_button);
            self.list.append(&row);
        }
    }
}
