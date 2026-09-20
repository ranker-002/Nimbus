use std::sync::Arc;

use tokio::sync::mpsc;

use nimbus_backend::orchestrator::Orchestrator;
use nimbus_core::events::{BackendCommand, UiEvent};
use nimbus_network::manager::NmManager;

/// Command queue depth. Commands are cheap and the UI only issues them on user
/// action, so a small buffer is plenty.
const COMMAND_QUEUE: usize = 64;
/// Event queue depth. The stats poller emits a couple of events a second, so
/// this leaves room for a brief stall in the UI without dropping anything.
const EVENT_QUEUE: usize = 256;

/// Owns the backend thread and the two channels the UI talks to it through.
///
/// Everything network-related runs on a dedicated Tokio runtime on its own
/// thread; the GTK main loop only ever sends [`BackendCommand`]s and receives
/// [`UiEvent`]s. Nothing here touches the network on construction — the app
/// must be able to start up without altering a single connection.
pub struct AppController {
    cmd_tx: mpsc::Sender<BackendCommand>,
    event_rx: async_channel::Receiver<UiEvent>,
}

impl AppController {
    pub fn spawn() -> Self {
        let (cmd_tx, mut cmd_rx) = mpsc::channel(COMMAND_QUEUE);
        // async-channel works on both sides of the thread boundary, so the GTK
        // loop can await events with glib::spawn_future_local.
        let (event_tx, event_rx) = async_channel::bounded(EVENT_QUEUE);

        std::thread::Builder::new()
            .name("nimbus-backend".into())
            .spawn(move || {
                let rt = match tokio::runtime::Runtime::new() {
                    Ok(rt) => rt,
                    Err(e) => {
                        log::error!("Failed to create Tokio runtime: {}", e);
                        return;
                    }
                };

                rt.block_on(async move {
                    let nm = Arc::new(NmManager::new().await);
                    let orch = Arc::new(Orchestrator::new(nm, event_tx));

                    // Runs until the UI drops the sender, i.e. until quit.
                    while let Some(cmd) = cmd_rx.recv().await {
                        orch.handle_command(cmd).await;
                    }
                });
            })
            .expect("failed to spawn backend thread");

        Self { cmd_tx, event_rx }
    }

    /// Queues a command for the backend. Never blocks the GTK main loop; a full
    /// queue means the backend is busy and the command is dropped with a log.
    pub fn send(&self, cmd: BackendCommand) {
        if let Err(e) = self.cmd_tx.try_send(cmd) {
            log::error!("Could not queue backend command: {}", e);
        }
    }

    /// Stream of backend events, to be consumed from the GTK main loop.
    pub fn events(&self) -> async_channel::Receiver<UiEvent> {
        self.event_rx.clone()
    }
}
