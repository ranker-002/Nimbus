use std::cell::RefCell;

use gtk::glib::subclass::prelude::*;
use gtk::glib;

mod imp {
    use super::*;
    use gtk::glib::Properties;
    use std::cell::Cell;

    #[derive(Properties, Default)]
    #[properties(wrapper_type = super::HotspotViewModel)]
    pub struct HotspotViewModel {
        #[property(get, set)]
        pub ssid: RefCell<String>,
        #[property(get, set)]
        pub password: RefCell<String>,
        #[property(get, set)]
        pub is_active: Cell<bool>,
        #[property(get, set)]
        pub status_text: RefCell<String>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for HotspotViewModel {
        const NAME: &'static str = "HotspotViewModel";
        type Type = super::HotspotViewModel;
    }

    #[glib::derived_properties]
    impl ObjectImpl for HotspotViewModel {}
}

glib::wrapper! {
    pub struct HotspotViewModel(ObjectSubclass<imp::HotspotViewModel>);
}

impl HotspotViewModel {
    pub fn new() -> Self {
        glib::Object::builder().build()
    }
}
