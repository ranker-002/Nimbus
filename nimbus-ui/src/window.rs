use adw::prelude::*;

use crate::app::NimbusApp;
use crate::navigation::NimbusNavigationView;

pub struct NimbusWindow;

impl NimbusWindow {
    pub fn create(app: &NimbusApp) -> adw::ApplicationWindow {
        let nav = NimbusNavigationView::new();

        let toolbar_view = adw::ToolbarView::new();
        let header = adw::HeaderBar::new();
        toolbar_view.add_top_bar(&header);
        toolbar_view.set_content(Some(nav.widget()));

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("Nimbus Hotspot")
            .default_width(900)
            .default_height(680)
            .content(&toolbar_view)
            .build();

        window.present();
        window
    }
}
