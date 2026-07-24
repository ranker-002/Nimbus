use tokio::sync::watch;

use nimbus_core::error::Result;
use nimbus_core::types::HotspotState;

pub struct HotspotMonitor {
    state_rx: watch::Receiver<HotspotState>,
}

impl HotspotMonitor {
    pub fn new(state_rx: watch::Receiver<HotspotState>) -> Self {
        Self { state_rx }
    }

    pub async fn wait_for_stop(&mut self) -> Result<()> {
        loop {
            if self.state_rx.changed().await.is_err() {
                break;
            }
            let state = self.state_rx.borrow().clone();
            if matches!(state, HotspotState::Inactive | HotspotState::Error(_)) {
                break;
            }
        }
        Ok(())
    }

    pub fn current_state(&self) -> HotspotState {
        self.state_rx.borrow().clone()
    }
}
