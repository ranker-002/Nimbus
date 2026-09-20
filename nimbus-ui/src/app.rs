use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gio, glib};

use crate::controller::AppController;
use crate::window::NimbusWindow;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct NimbusApp {
        pub controller: RefCell<Option<Rc<AppController>>>,
        /// Kept alive for the lifetime of the application: the window owns the
        /// pages and the event pump that feeds them.
        pub window: RefCell<Option<Rc<NimbusWindow>>>,
        /// Set when the application was launched by the login autostart entry.
        pub autostart: Cell<bool>,
    }

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
            app.setup_css();

            if let Some(window) = self.window.borrow().as_ref() {
                window.present();
                return;
            }

            let Some(controller) = self.controller.borrow().clone() else {
                log::error!("No controller: build the application with NimbusApp::new");
                return;
            };

            let window = NimbusWindow::build(&app, controller, self.autostart.get());
            window.present();
            self.window.replace(Some(window));
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

impl NimbusApp {
    pub fn new(controller: AppController) -> Self {
        let app: Self = glib::Object::builder()
            .property("application-id", nimbus_core::constants::APP_ID)
            .property("flags", gio::ApplicationFlags::FLAGS_NONE)
            .property("resource-base-path", "/com/nimbus/Hotspot/")
            .build();
        app.imp().controller.replace(Some(Rc::new(controller)));
        glib::set_application_name(nimbus_core::constants::APP_NAME);
        app
    }

    pub fn setup_css(&self) {
        let provider = gtk::CssProvider::new();
        provider.load_from_resource("/com/nimbus/Hotspot/style.css");

        let Some(display) = gtk::gdk::Display::default() else {
            log::error!("No display available; skipping stylesheet");
            return;
        };
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        // Makes the application icon available even when running from a build
        // tree rather than an installed one.
        gtk::IconTheme::for_display(&display).add_resource_path("/com/nimbus/Hotspot/icons");
    }

    fn setup_actions(&self) {
        let quit_action = gio::ActionEntry::builder("quit")
            .activate(move |app: &Self, _, _| app.quit())
            .build();

        let about_action = gio::ActionEntry::builder("about")
            .activate(move |app: &Self, _, _| app.show_about())
            .build();

        let shortcuts_action = gio::ActionEntry::builder("shortcuts")
            .activate(move |app: &Self, _, _| app.show_shortcuts())
            .build();

        self.add_action_entries([quit_action, about_action, shortcuts_action]);
        self.set_accels_for_action("app.quit", &["<primary>q"]);
        self.set_accels_for_action("app.shortcuts", &["<primary>question"]);
    }

    fn show_about(&self) {
        let about = adw::AboutDialog::builder()
            .application_name(nimbus_core::constants::APP_NAME)
            .application_icon(nimbus_core::constants::APP_ID)
            .developer_name("Nimbus Contributors")
            .version(nimbus_core::constants::APP_VERSION)
            .website("https://github.com/ranker-002/Nimbus")
            .issue_url("https://github.com/ranker-002/Nimbus/issues")
            .license_type(gtk::License::MitX11)
            .developers(vec!["Nimbus Contributors"])
            .build();

        let parent = self.active_window();
        about.present(parent.as_ref());
    }

    fn show_shortcuts(&self) {
        const SHORTCUTS: &str = r#"
        <interface>
          <object class="GtkShortcutsWindow" id="shortcuts">
            <property name="modal">True</property>
            <child>
              <object class="GtkShortcutsSection">
                <property name="section-name">main</property>
                <property name="max-height">10</property>
                <child>
                  <object class="GtkShortcutsGroup">
                    <property name="title">General</property>
                    <child>
                      <object class="GtkShortcutsShortcut">
                        <property name="title">Quit Nimbus</property>
                        <property name="accelerator">&lt;Primary&gt;q</property>
                      </object>
                    </child>
                    <child>
                      <object class="GtkShortcutsShortcut">
                        <property name="title">Keyboard shortcuts</property>
                        <property name="accelerator">&lt;Primary&gt;question</property>
                      </object>
                    </child>
                  </object>
                </child>
              </object>
            </child>
          </object>
        </interface>
        "#;

        let builder = gtk::Builder::from_string(SHORTCUTS);
        let Some(window) = builder.object::<gtk::ShortcutsWindow>("shortcuts") else {
            log::warn!("Could not build the shortcuts window");
            return;
        };
        window.set_transient_for(self.active_window().as_ref());
        window.present();
    }

    pub fn run(&self) {
        self.setup_actions();

        // `--autostart` is consumed here rather than rejected by GApplication:
        // the login entry passes it so Nimbus can start the last hotspot only
        // when the user asked for it in Settings.
        let mut args: Vec<String> = std::env::args().collect();
        let autostart = args.iter().any(|arg| arg == "--autostart");
        args.retain(|arg| arg != "--autostart");
        self.imp().autostart.set(autostart);

        gio::prelude::ApplicationExtManual::run_with_args(self, &args);
    }
}

impl Default for NimbusApp {
    fn default() -> Self {
        panic!("NimbusApp::default() is not supported; use NimbusApp::new(controller)")
    }
}
