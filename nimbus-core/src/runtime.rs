use std::sync::OnceLock;
use tokio::runtime::Runtime;

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

pub fn runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| Runtime::new().expect("Failed to create Tokio runtime"))
}

#[macro_export]
macro_rules! spawn {
    ($model:expr, $f:expr) => {
        $crate::runtime::runtime().spawn(glib::clone!(
            #[strong(rename_to = model)]
            $model,
            async move {
                let mut model = model.lock().await;
                let Err(e) = ($f)(&mut model).await else {
                    return;
                };
                model
                    .send_event($crate::events::UiEvent::ErrorOccurred(e.to_string()))
                    .await;
            }
        ));
    };
}
