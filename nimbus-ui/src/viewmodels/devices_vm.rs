use std::cell::RefCell;

use gtk::glib::subclass::prelude::*;
use gtk::glib;

use nimbus_core::types::StationInfo;

mod imp {
    use super::*;
    use gtk::glib::Properties;

    #[derive(Properties, Default)]
    #[properties(wrapper_type = super::DevicesViewModel)]
    pub struct DevicesViewModel {
        #[property(get = Self::stations_json, set = Self::set_stations_json)]
        pub stations_json: RefCell<String>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for DevicesViewModel {
        const NAME: &'static str = "DevicesViewModel";
        type Type = super::DevicesViewModel;
    }

    #[glib::derived_properties]
    impl ObjectImpl for DevicesViewModel {}
}

impl imp::DevicesViewModel {
    fn stations_json(&self) -> String {
        self.stations_json.borrow().clone()
    }

    fn set_stations_json(&self, value: String) {
        *self.stations_json.borrow_mut() = value;
    }
}

glib::wrapper! {
    pub struct DevicesViewModel(ObjectSubclass<imp::DevicesViewModel>);
}

impl DevicesViewModel {
    pub fn new() -> Self {
        glib::Object::builder().build()
    }

    pub fn update_stations(&self, stations: &[StationInfo]) {
        if let Ok(json) = serde_json::to_string(stations) {
            self.set_stations_json(json);
        }
    }
}
