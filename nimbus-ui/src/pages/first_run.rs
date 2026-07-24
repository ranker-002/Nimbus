use adw::prelude::*;
use gtk::glib;

pub struct FirstRunPage {
    main_box: gtk::Box,
    start_button: gtk::Button,
}

impl FirstRunPage {
    pub fn new() -> Self {
        let main_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(24)
            .valign(gtk::Align::Center)
            .halign(gtk::Align::Center)
            .margin_top(48)
            .margin_bottom(48)
            .margin_start(48)
            .margin_end(48)
            .build();

        let icon = gtk::Image::from_icon_name("network-wireless-symbolic");
        icon.set_pixel_size(128);
        icon.add_css_class("accent");
        main_box.append(&icon);

        let title = gtk::Label::new(Some("Welcome to Nimbus Hotspot"));
        title.add_css_class("title-1");
        title.set_halign(gtk::Align::Center);
        main_box.append(&title);

        let subtitle = gtk::Label::new(Some(
            "Transform your Linux computer into a Wi-Fi hotspot.\n\
             Nimbus will detect your Wi-Fi adapter and configure everything automatically.",
        ));
        subtitle.set_halign(gtk::Align::Center);
        subtitle.set_justify(gtk::Justification::Center);
        subtitle.add_css_class("dim-label");
        main_box.append(&subtitle);

        let start_button = gtk::Button::builder()
            .label("Get Started")
            .css_classes(["suggested-action", "pill"])
            .halign(gtk::Align::Center)
            .build();
        main_box.append(&start_button);

        Self {
            main_box,
            start_button,
        }
    }

    pub fn widget(&self) -> &gtk::Widget {
        self.main_box.upcast_ref()
    }

    pub fn connect_start<F: Fn() + 'static>(&self, f: F) {
        self.start_button.connect_clicked(move |_| f());
    }
}
