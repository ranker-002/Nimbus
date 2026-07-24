use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gio, glib};

use crate::window::NimbusWindow;

mod imp {
    use super::*;

    #[derive(Debug, Default)]
    pub struct NimbusApp;

    #[glib::object_subclass]
    impl ObjectSubclass for NimbusApp {
        const NAME: &'static str = "NimbusApp";
        type Type = super::NimbusApp;
        type ParentType = adw::Application;
    }

    impl ObjectImpl for NimbusApp {}
    impl ApplicationImpl for NimbusApp {
        fn activate(&self) {
            let app = self.obj();
            let window = if let Some(w) = app.active_window() {
                w
            } else {
                NimbusWindow::create(&app).upcast()
            };
            window.present();
        }
    }
    impl GtkApplicationImpl for NimbusApp {}
    impl AdwApplicationImpl for NimbusApp {}
}

glib::wrapper! {
    pub struct NimbusApp(ObjectSubclass<imp::NimbusApp>)
        @extends gio::Application, gtk::Application, adw::Application,
        @implements gio::ActionGroup, gio::ActionMap;
}

impl Default for NimbusApp {
    fn default() -> Self {
        Self::new()
    }
}

impl NimbusApp {
    pub fn new() -> Self {
        glib::Object::builder()
            .property("application-id", nimbus_core::constants::APP_ID)
            .property("flags", gio::ApplicationFlags::FLAGS_NONE)
            .property("resource-base-path", "/com/nimbus/Hotspot/")
            .build()
    }

    pub fn run(&self) {
        self.setup_css();
        self.setup_actions();
        gio::prelude::ApplicationExtManual::run(self);
    }

    fn setup_css(&self) {
        let provider = gtk::CssProvider::new();
        provider.load_from_resource("/com/nimbus/Hotspot/style.css");

        gtk::style_context_add_provider_for_display(
            &gtk::gdk::Display::default().expect("Could not get default display"),
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        adw::StyleManager::default()
            .set_color_scheme(adw::ColorScheme::ForceDark);
    }

    fn setup_actions(&self) {
        let quit_action = gio::ActionEntry::builder("quit")
            .activate(move |app: &Self, _, _| app.quit())
            .build();
        self.add_action_entries([quit_action]);
        self.set_accels_for_action("app.quit", &["<primary>q"]);
    }
}
