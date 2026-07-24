use adw::prelude::*;
use gtk::glib;

use nimbus_core::types::HotspotConfig;
use nimbus_wifi::qr::generate_wifi_qr;

pub fn show_qr_dialog(parent: &impl IsA<gtk::Widget>, config: &HotspotConfig) {
    let dialog = adw::Dialog::builder()
        .title("Wi-Fi QR Code")
        .content_width(350)
        .content_height(450)
        .build();

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(16)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .valign(gtk::Align::Center)
        .halign(gtk::Align::Center)
        .build();

    match generate_wifi_qr(
        &config.ssid,
        &config.password,
        &config.security,
        config.hidden,
    ) {
        Ok(svg) => {
            let svg_widget = gtk::Picture::builder()
                .content_fit(gtk::ContentFit::Contain)
                .build();

            let bytes = glib::Bytes::from(svg.as_bytes());
            let texture = gtk::gdk::Texture::from_bytes(&bytes).ok();

            if let Some(texture) = texture {
                svg_widget.set_paintable(Some(&texture));
            }

            content.append(&svg_widget);
        }
        Err(e) => {
            let error_label = gtk::Label::new(Some(&format!("Failed to generate QR: {}", e)));
            error_label.add_css_class("error");
            content.append(&error_label);
        }
    }

    let ssid_label = gtk::Label::new(Some(&format!("Network: {}", config.ssid)));
    ssid_label.add_css_class("heading");
    content.append(&ssid_label);

    let close_button = gtk::Button::builder()
        .label("Close")
        .css_classes(["flat"])
        .halign(gtk::Align::Center)
        .build();
    content.append(&close_button);

    dialog.set_child(Some(&content));
    let dialog_clone = dialog.clone();
    close_button.connect_clicked(move |_| { dialog_clone.close(); });
    dialog.present(Some(parent));
}
