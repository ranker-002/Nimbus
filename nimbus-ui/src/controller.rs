use std::sync::Arc;

use tokio::sync::mpsc;

use nimbus_backend::orchestrator::Orchestrator;
use nimbus_core::events::{BackendCommand, ToastKind, UiEvent};
use nimbus_network::manager::NmManager;

use crate::service::SystemService;

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
///
/// When the privileged D-Bus service is running, operations that need root
/// (`hostapd`, `dnsmasq`, `nft`, `iw reg set`, scanning, kicking a client) are
/// routed to it instead of being attempted in-process. Everything else keeps
/// using the local backend, which also serves as the fallback when the service
/// is not installed.
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
                    let orchestrator = Arc::new(Orchestrator::new(nm, event_tx.clone()));

                    let service = SystemService::connect().await;
                    match &service {
                        Some(service) => match service.forward_events(event_tx.clone()).await {
                            Ok(()) => log::info!(
                                "Nimbus D-Bus service found; privileged operations run there"
                            ),
                            Err(e) => log::warn!(
                                "Could not subscribe to the D-Bus service ({}); \
                                     privileged operations are unavailable",
                                e
                            ),
                        },
                        None => {
                            log::info!("Nimbus D-Bus service not running; using the local backend")
                        }
                    }

                    // Runs until the UI drops the sender, i.e. until quit.
                    while let Some(cmd) = cmd_rx.recv().await {
                        match &service {
                            Some(service) => {
                                handle_service_command(service, &orchestrator, &event_tx, cmd).await
                            }
                            None => orchestrator.handle_command(cmd).await,
                        }
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

/// Runs one command against the privileged service, falling back to the local
/// backend for everything the user may do without privileges.
async fn handle_service_command(
    service: &SystemService,
    orchestrator: &Arc<Orchestrator>,
    event_tx: &async_channel::Sender<UiEvent>,
    cmd: BackendCommand,
) {
    match cmd {
        BackendCommand::StartHotspot {
            config,
            interface,
            confirmed,
        } => {
            if confirmed {
                match service.start(&config, interface.as_deref()).await {
                    Ok(()) => toast(event_tx, "Hotspot started", ToastKind::Success).await,
                    Err(e) => emit(event_tx, UiEvent::ErrorOccurred(e)).await,
                }
                return;
            }

            // The plan must come from the service: only it can tell whether
            // the shared backend is available once privileges are in play.
            match service.plan(&config, interface.as_deref()).await {
                Ok(plan) if plan.needs_confirmation() => {
                    let reason = plan
                        .warnings
                        .first()
                        .cloned()
                        .unwrap_or_else(|| "This will disconnect your Wi-Fi.".into());
                    emit(event_tx, UiEvent::ConfirmationRequired { plan, reason }).await;
                }
                Ok(_) => match service.start(&config, interface.as_deref()).await {
                    Ok(()) => toast(event_tx, "Hotspot started", ToastKind::Success).await,
                    Err(e) => emit(event_tx, UiEvent::ErrorOccurred(e)).await,
                },
                Err(e) => emit(event_tx, UiEvent::ErrorOccurred(e)).await,
            }
        }
        BackendCommand::StopHotspot => match service.stop().await {
            Ok(()) => toast(event_tx, "Hotspot stopped", ToastKind::Info).await,
            Err(e) => emit(event_tx, UiEvent::ErrorOccurred(e)).await,
        },
        BackendCommand::RefreshStatus => match service.status().await {
            Ok(state) => emit(event_tx, UiEvent::HotspotStateChanged(state)).await,
            Err(e) => emit(event_tx, UiEvent::ErrorOccurred(e)).await,
        },
        BackendCommand::ScanNetworks => match service.scan().await {
            Ok(networks) => emit(event_tx, UiEvent::ScannedNetworks(networks)).await,
            Err(e) => {
                emit(
                    event_tx,
                    UiEvent::ShowToast {
                        message: format!("Scan failed: {}", e),
                        kind: ToastKind::Error,
                    },
                )
                .await
            }
        },
        BackendCommand::DisconnectStation { mac } => {
            if let Err(e) = service.disconnect(mac).await {
                emit(event_tx, UiEvent::ErrorOccurred(e)).await;
            }
        }
        BackendCommand::GetHistory { limit } => match service.history(limit as u32).await {
            Ok(records) => emit(event_tx, UiEvent::History(records)).await,
            Err(e) => log::warn!("Could not read history from the service: {}", e),
        },
        // Read-only queries answered faster (and just as correctly) in-process.
        other => orchestrator.handle_command(other).await,
    }
}

async fn emit(event_tx: &async_channel::Sender<UiEvent>, event: UiEvent) {
    if event_tx.send(event).await.is_err() {
        log::debug!("UI event dropped: no receiver");
    }
}

async fn toast(
    event_tx: &async_channel::Sender<UiEvent>,
    message: impl Into<String>,
    kind: ToastKind,
) {
    emit(
        event_tx,
        UiEvent::ShowToast {
            message: message.into(),
            kind,
        },
    )
    .await;
}
