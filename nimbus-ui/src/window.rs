use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::gio;
use gtk::glib;

use nimbus_core::events::{BackendCommand, ToastKind, UiEvent};
use nimbus_core::types::{
    HotspotBackend, HotspotConfig, HotspotPlan, HotspotState, NetworkInterface, ScannedNetwork,
};
use nimbus_settings::hotspots;
use nimbus_settings::preferences::{load_preferences, save_preferences, AppPreferences};

use crate::app::NimbusApp;
use crate::components::qr_dialog::show_qr_dialog;
use crate::controller::AppController;
use crate::navigation::{self, NavEntry, Navigation};
use crate::pages::dashboard::DashboardPage;
use crate::pages::devices::DevicesPage;
use crate::pages::first_run::FirstRunPage;
use crate::pages::hotspot::HotspotPage;
use crate::pages::settings::SettingsPage;

/// Owns the widget tree and translates backend events into UI updates.
pub struct NimbusWindow {
    window: adw::ApplicationWindow,
    toasts: adw::ToastOverlay,
    navigation: Navigation,
    dashboard: DashboardPage,
    hotspot: Rc<HotspotPage>,
    devices: DevicesPage,
    controller: Rc<AppController>,
    /// The config last handed to the backend, so the QR code and any
    /// confirmation retry describe the hotspot the user actually asked for.
    pending_config: RefCell<HotspotConfig>,
    prefs: RefCell<AppPreferences>,
    /// Wi-Fi adapters as last reported, used to answer "Automatic" with a
    /// concrete interface when querying capabilities.
    interfaces: RefCell<Vec<NetworkInterface>>,
}

impl NimbusWindow {
    pub fn build(app: &NimbusApp, controller: Rc<AppController>, autostart: bool) -> Rc<Self> {
        let dashboard = DashboardPage::new();
        let hotspot = Rc::new(HotspotPage::new());
        let devices = DevicesPage::new();
        let settings = SettingsPage::new();
        let first_run = FirstRunPage::new();

        let preferences = load_preferences();
        hotspot.apply_defaults(&defaults_from(&preferences));
        apply_appearance(&preferences);

        let show_welcome = !nimbus_settings::preferences::preferences_exist()
            && hotspots::load_hotspots().unwrap_or_default().is_empty();

        let mut entries = Vec::new();
        if show_welcome {
            entries.push(NavEntry {
                tag: "welcome",
                title: "Welcome",
                icon: "starred-symbolic",
                widget: first_run.widget(),
            });
        }
        entries.push(NavEntry {
            tag: "dashboard",
            title: "Dashboard",
            icon: "view-grid-symbolic",
            widget: dashboard.widget(),
        });
        entries.push(NavEntry {
            tag: "hotspot",
            title: "Hotspot",
            icon: "network-wireless-symbolic",
            widget: hotspot.widget(),
        });
        entries.push(NavEntry {
            tag: "devices",
            title: "Devices",
            icon: "computer-symbolic",
            widget: devices.widget(),
        });
        entries.push(NavEntry {
            tag: "settings",
            title: "Settings",
            icon: "emblem-system-symbolic",
            widget: settings.widget(),
        });
        let navigation = navigation::build(&entries);

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("Nimbus Hotspot")
            .default_width(900)
            .default_height(680)
            .build();

        let this = Rc::new(Self {
            window,
            toasts: adw::ToastOverlay::new(),
            navigation,
            dashboard,
            hotspot: Rc::clone(&hotspot),
            devices,
            controller,
            pending_config: RefCell::new(HotspotConfig::default()),
            prefs: RefCell::new(preferences),
            interfaces: RefCell::new(Vec::new()),
        });

        this.build_layout();
        this.connect_actions();
        this.pump_events();

        // The first-run page must be able to send the user to the form.
        {
            let this = Rc::clone(&this);
            first_run.connect_start(move || this.navigation.show("hotspot"));
        }

        // Preferences changed: keep the form defaults, the colour scheme and
        // the notification setting in step without a restart.
        {
            let this = Rc::clone(&this);
            settings.connect_changed(move |prefs| {
                this.hotspot.apply_defaults(&defaults_from(prefs));
                apply_appearance(prefs);
                *this.prefs.borrow_mut() = prefs.clone();
            });
        }

        this.hotspot.connect_notice({
            let this = Rc::clone(&this);
            move |message, kind| this.toast(message, kind)
        });

        // Read-only startup queries: list the adapters and pick up a hotspot
        // that may already be running. Neither changes any network state.
        this.controller.send(BackendCommand::DetectInterfaces);
        this.controller.send(BackendCommand::RefreshStatus);
        this.controller.send(BackendCommand::GetRegulatoryDomain);
        this.controller
            .send(BackendCommand::GetHistory { limit: 10 });

        if autostart && this.prefs.borrow().auto_start {
            if let Some(config) = this.prefs.borrow().last_config.clone() {
                this.controller.send(BackendCommand::StartHotspot {
                    config,
                    interface: None,
                    confirmed: false,
                });
            }
        }

        this
    }

    pub fn present(&self) {
        self.window.present();
    }

    fn build_layout(self: &Rc<Self>) {
        let header = adw::HeaderBar::new();

        let menu = gio::Menu::new();
        menu.append(Some("About Nimbus"), Some("app.about"));
        menu.append(Some("Keyboard Shortcuts"), Some("app.shortcuts"));
        menu.append(Some("Quit"), Some("app.quit"));
        let menu_button = gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .menu_model(&menu)
            .build();
        header.pack_end(&menu_button);

        let toolbar_view = adw::ToolbarView::new();
        toolbar_view.add_top_bar(&header);
        toolbar_view.set_content(Some(self.navigation.widget()));

        // Toasts need to sit above the whole content area.
        self.toasts.set_child(Some(&toolbar_view));
        self.window.set_content(Some(&self.toasts));
    }

    fn connect_actions(self: &Rc<Self>) {
        let start = Rc::clone(self);
        self.hotspot.connect_start(move || start.request_start());

        let stop = Rc::clone(self);
        self.hotspot.connect_stop(move || stop.confirm_stop());

        let qr = Rc::clone(self);
        self.hotspot.connect_qr(move || {
            let config = qr.hotspot.get_config();
            if config.ssid.is_empty() {
                qr.toast("Enter a network name first", ToastKind::Warning);
                return;
            }
            show_qr_dialog(&qr.window, &config);
        });

        let scan = Rc::clone(self);
        self.hotspot
            .connect_scan(move || scan.controller.send(BackendCommand::ScanNetworks));

        let kick = Rc::clone(self);
        self.devices.connect_kick(move |mac| {
            kick.controller
                .send(BackendCommand::DisconnectStation { mac })
        });

        let caps = Rc::clone(self);
        self.hotspot.connect_interface_selected(move |interface| {
            let interface = interface.or_else(|| {
                caps.interfaces
                    .borrow()
                    .first()
                    .map(|iface| iface.name.clone())
            });
            if let Some(interface) = interface {
                caps.controller
                    .send(BackendCommand::GetAdapterInfo { interface });
            }
        });

        self.hotspot.wire();
        self.devices.update_stations(&[]);
    }

    /// Asks the backend to start a hotspot. The backend answers with
    /// [`UiEvent::ConfirmationRequired`] instead of acting if doing so would
    /// drop the machine's Wi-Fi connection.
    fn request_start(&self) {
        let config = self.hotspot.get_config();
        self.hotspot.update_validation_hint();
        if let Err(e) = config.validate() {
            self.toast(e.to_string(), ToastKind::Error);
            return;
        }

        // Remember what was started, for auto-start on the next login.
        {
            let mut prefs = self.prefs.borrow_mut();
            prefs.last_config = Some(config.clone());
            if let Err(e) = save_preferences(&prefs) {
                log::warn!("Could not save the last used hotspot: {}", e);
            }
        }

        *self.pending_config.borrow_mut() = config.clone();
        self.controller.send(BackendCommand::StartHotspot {
            config,
            interface: self.hotspot.selected_interface(),
            confirmed: false,
        });
    }

    fn confirm_stop(self: &Rc<Self>) {
        let dialog = adw::AlertDialog::builder()
            .heading("Stop the hotspot?")
            .body("Connected devices will be disconnected.")
            .build();
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("stop", "Stop");
        dialog.set_response_appearance("stop", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");

        let this = Rc::clone(self);
        dialog.connect_response(None, move |_, response| {
            if response == "stop" {
                this.controller.send(BackendCommand::StopHotspot);
            }
        });
        dialog.present(Some(&self.window));
    }

    /// Drains backend events on the GTK main loop for as long as the window
    /// lives.
    fn pump_events(self: &Rc<Self>) {
        let this = Rc::clone(self);
        let events = self.controller.events();

        glib::spawn_future_local(async move {
            while let Ok(event) = events.recv().await {
                this.handle_event(event);
            }
            log::debug!("Backend event stream closed");
        });
    }

    fn handle_event(self: &Rc<Self>, event: UiEvent) {
        match event {
            UiEvent::HotspotStateChanged(state) => {
                self.hotspot.set_state(&state);
                self.dashboard.set_status(&state);

                match &state {
                    HotspotState::Inactive | HotspotState::Error(_) => {
                        self.dashboard.clear_stats();
                        self.devices.update_stations(&[]);
                    }
                    _ => {}
                }

                match &state {
                    HotspotState::Active(ssid) => {
                        self.notify("Hotspot started", &format!("'{}' is running", ssid));
                    }
                    HotspotState::Inactive => {
                        self.notify("Hotspot stopped", "The hotspot is no longer running.");
                        self.controller
                            .send(BackendCommand::GetHistory { limit: 10 });
                    }
                    HotspotState::Error(message) => {
                        self.notify("Hotspot error", message);
                    }
                    _ => {}
                }
            }
            UiEvent::StatsUpdated(stats) => self.dashboard.update_stats(&stats),
            UiEvent::StationsUpdated(stations) => self.devices.update_stations(&stations),
            UiEvent::InterfacesDetected(interfaces) => {
                *self.interfaces.borrow_mut() = interfaces
                    .iter()
                    .filter(|iface| iface.interface_type == nimbus_core::types::InterfaceType::Wifi)
                    .cloned()
                    .collect();
                self.hotspot.set_interfaces(&interfaces);

                // Show what the first adapter can do even before the user
                // picks one.
                if let Some(first) = self.interfaces.borrow().first() {
                    let name = first.name.clone();
                    self.controller
                        .send(BackendCommand::GetAdapterInfo { interface: name });
                }
            }
            UiEvent::AdapterInfo(caps) => self.hotspot.set_capabilities(&caps),
            UiEvent::ScannedNetworks(networks) => self.show_scan_dialog(&networks),
            UiEvent::History(records) => self.dashboard.show_history(&records),
            UiEvent::RegulatoryDomain(domain) => {
                self.hotspot.set_regulatory_domain(domain.as_deref());
            }
            UiEvent::ConfirmationRequired { plan, reason } => {
                self.confirm_disconnect(plan, reason);
            }
            UiEvent::ShowToast { message, kind } => self.toast(message, kind),
            UiEvent::ErrorOccurred(message) => self.toast(message, ToastKind::Error),
        }
    }

    /// Asks before doing anything that costs the user their connection. The
    /// backend has changed nothing at this point.
    fn confirm_disconnect(self: &Rc<Self>, plan: HotspotPlan, reason: String) {
        let mut body = reason;
        for warning in plan.warnings.iter().skip(1) {
            body.push_str("\n\n");
            body.push_str(warning);
        }

        let dialog = adw::AlertDialog::builder()
            .heading("Disconnect from Wi-Fi?")
            .body(body)
            .build();
        dialog.add_response("cancel", "Cancel");

        // Sharing without dropping the connection needs root; offer to restart
        // the whole application privileged rather than silently disconnecting.
        let can_retry_as_root = plan.backend == HotspotBackend::NetworkManager
            && plan
                .share_blockers
                .iter()
                .any(|blocker| blocker.contains("administrator rights"));
        if can_retry_as_root {
            dialog.add_response("root", "Restart as Administrator");
        }

        dialog.add_response("start", "Start Anyway");
        dialog.set_response_appearance("start", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");

        let this = Rc::clone(self);
        dialog.connect_response(None, move |_, response| match response {
            "start" => this.controller.send(BackendCommand::StartHotspot {
                config: this.pending_config.borrow().clone(),
                interface: Some(plan.ap_interface.clone()),
                confirmed: true,
            }),
            "root" => this.relaunch_as_root(),
            _ => {}
        });

        dialog.present(Some(&self.window));
    }

    /// Re-runs the application through `pkexec` so the shared backend — which
    /// needs hostapd, dnsmasq, nft and `iw reg set` — becomes available.
    fn relaunch_as_root(self: &Rc<Self>) {
        let Ok(exe) = std::env::current_exe() else {
            self.toast(
                "Could not determine the Nimbus executable",
                ToastKind::Error,
            );
            return;
        };

        let mut command = std::process::Command::new("pkexec");
        command.arg("env");
        // pkexec starts a clean environment; carry over what the session needs
        // so the privileged window can still appear on the user's display.
        for var in [
            "DISPLAY",
            "WAYLAND_DISPLAY",
            "XDG_RUNTIME_DIR",
            "XDG_SESSION_TYPE",
            "XAUTHORITY",
            "DBUS_SESSION_BUS_ADDRESS",
        ] {
            if let Ok(value) = std::env::var(var) {
                command.arg(format!("{}={}", var, value));
            }
        }
        command.arg(exe);

        match command.spawn() {
            Ok(_) => {
                self.toast(
                    "Restarting Nimbus with administrator rights…",
                    ToastKind::Info,
                );
                if let Some(application) = self.window.application() {
                    application.quit();
                }
            }
            Err(e) => self.toast(
                format!("Could not restart as administrator: {}", e),
                ToastKind::Error,
            ),
        }
    }

    /// Lists nearby networks, newest scan first, and lets the user pin the
    /// channel of one.
    fn show_scan_dialog(self: &Rc<Self>, networks: &[ScannedNetwork]) {
        if networks.is_empty() {
            self.toast("No nearby networks found", ToastKind::Info);
            return;
        }

        let mut sorted: Vec<&ScannedNetwork> = networks.iter().collect();
        sorted.sort_by_key(|network| std::cmp::Reverse(network.signal_dbm));

        let dialog = adw::Dialog::builder()
            .title("Nearby networks")
            .content_width(420)
            .content_height(500)
            .build();

        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .build();

        let scrolled = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .build();
        let list = gtk::ListBox::builder().css_classes(["boxed-list"]).build();
        scrolled.set_child(Some(&list));
        content.append(&scrolled);

        for network in sorted {
            let title = if network.ssid.is_empty() {
                "Hidden network".to_string()
            } else {
                network.ssid.clone()
            };
            let row = adw::ActionRow::builder()
                .title(&title)
                .subtitle(format!(
                    "Channel {} · {} MHz · {} dBm",
                    network.channel, network.frequency, network.signal_dbm
                ))
                .activatable(network.channel > 0)
                .build();
            row.add_suffix(&gtk::Image::from_icon_name(
                "network-wireless-signal-good-symbolic",
            ));

            let this = Rc::clone(self);
            let dialog_clone = dialog.clone();
            let channel = network.channel;
            row.connect_activated(move |_| {
                this.hotspot.set_channel(channel);
                dialog_clone.close();
            });

            list.append(&row);
        }

        let close_button = gtk::Button::builder()
            .label("Close")
            .css_classes(["flat"])
            .halign(gtk::Align::Center)
            .build();
        let dialog_clone = dialog.clone();
        close_button.connect_clicked(move |_| {
            dialog_clone.close();
        });
        content.append(&close_button);

        dialog.set_child(Some(&content));
        dialog.present(Some(&self.window));
    }

    fn notify(&self, title: &str, body: &str) {
        if !self.prefs.borrow().show_notifications {
            return;
        }
        let notification = gio::Notification::new(title);
        notification.set_body(Some(body));
        if let Some(application) = self.window.application() {
            application.send_notification(None, &notification);
        }
    }

    fn toast(&self, message: impl AsRef<str>, kind: ToastKind) {
        let toast = adw::Toast::new(message.as_ref());
        toast.set_timeout(match kind {
            ToastKind::Error | ToastKind::Warning => 8,
            _ => 4,
        });
        self.toasts.add_toast(toast);
    }
}

fn apply_appearance(prefs: &AppPreferences) {
    let scheme = if prefs.dark_mode_only {
        adw::ColorScheme::ForceDark
    } else {
        adw::ColorScheme::Default
    };
    adw::StyleManager::default().set_color_scheme(scheme);
}

fn defaults_from(prefs: &AppPreferences) -> HotspotConfig {
    HotspotConfig {
        band: prefs.default_band.clone(),
        security: prefs.default_security.clone(),
        // 0 in preferences means "no limit".
        max_clients: match prefs.default_max_clients {
            0 => None,
            n => Some(n),
        },
        country_code: prefs.default_country.clone(),
        ..Default::default()
    }
}
