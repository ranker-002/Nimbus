fn main() {
    env_logger::init();
    let app = nimbus_ui::app::NimbusApp::new();
    app.run();
}
