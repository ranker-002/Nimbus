use gtk::gio;

use nimbus_ui::app::NimbusApp;
use nimbus_ui::controller::AppController;

fn main() {
    env_logger::init();

    let resource_data = include_bytes!(concat!(env!("OUT_DIR"), "/nimbus-hotspot.gresource"));
    let resource = gio::Resource::from_data(&gtk::glib::Bytes::from(resource_data.as_ref()))
        .expect("Failed to create resource");
    gio::resources_register(&resource);

    let controller = AppController::spawn();
    let app = NimbusApp::new(controller);
    app.run();
}
