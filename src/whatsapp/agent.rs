//! WhatsApp Agent
//!
//! Agent struct and startup logic. Mirrors the Telegram agent pattern.

use super::handler;
use crate::config::VoiceConfig;
use crate::llm::agent::AgentService;
use crate::services::{ServiceContext, SessionService};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

use super::sqlx_store::SqlxStore;
use wacore::types::events::Event;
use whatsapp_rust::bot::Bot;
use whatsapp_rust_tokio_transport::TokioWebSocketTransportFactory;
use whatsapp_rust_ureq_http_client::UreqHttpClient;

/// WhatsApp agent that forwards messages to the AgentService
pub struct WhatsAppAgent {
    agent_service: Arc<AgentService>,
    session_service: SessionService,
    allowed_phones: HashSet<String>,
    voice_config: VoiceConfig,
    shared_session_id: Arc<Mutex<Option<Uuid>>>,
}

impl WhatsAppAgent {
    pub fn new(
        agent_service: Arc<AgentService>,
        service_context: ServiceContext,
        allowed_phones: Vec<String>,
        voice_config: VoiceConfig,
        shared_session_id: Arc<Mutex<Option<Uuid>>>,
    ) -> Self {
        Self {
            agent_service,
            session_service: SessionService::new(service_context),
            allowed_phones: allowed_phones.into_iter().collect(),
            voice_config,
            shared_session_id,
        }
    }

    /// Start as a background task. Returns JoinHandle.
    /// If already paired (session.db exists), reconnects silently.
    /// If not paired, QR events are logged.
    pub fn start(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let db_path = crate::config::opencrabs_home()
                .join("whatsapp")
                .join("session.db");

            // Ensure parent directory exists
            if let Some(parent) = db_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }

            let backend = match SqlxStore::new(db_path.to_string_lossy().as_ref()).await {
                Ok(store) => Arc::new(store),
                Err(e) => {
                    tracing::error!("WhatsApp: failed to open session store: {}", e);
                    return;
                }
            };

            tracing::info!(
                "WhatsApp agent running (STT={}, TTS={})",
                self.voice_config.stt_enabled,
                self.voice_config.tts_enabled,
            );

            let agent = self.agent_service.clone();
            let session_svc = self.session_service.clone();
            let allowed = Arc::new(self.allowed_phones);
            let voice_config = Arc::new(self.voice_config);
            let shared_session = self.shared_session_id.clone();
            let extra_sessions: Arc<Mutex<HashMap<String, Uuid>>> =
                Arc::new(Mutex::new(HashMap::new()));

            let bot_result = Bot::builder()
                .with_backend(backend)
                .with_transport_factory(TokioWebSocketTransportFactory::new())
                .with_http_client(UreqHttpClient::new())
                .on_event(move |event, client| {
                    let agent = agent.clone();
                    let session_svc = session_svc.clone();
                    let allowed = allowed.clone();
                    let extra_sessions = extra_sessions.clone();
                    let voice_config = voice_config.clone();
                    let shared_session = shared_session.clone();
                    async move {
                        match event {
                            Event::PairingQrCode { ref code, .. } => {
                                tracing::info!(
                                    "WhatsApp: QR code available (scan with your phone)"
                                );
                                // In static mode, just log — QR display is handled by the connect tool
                                tracing::debug!("WhatsApp QR: {}", code);
                            }
                            Event::Connected(_) => {
                                tracing::info!("WhatsApp: connected successfully");
                            }
                            Event::PairSuccess(_) => {
                                tracing::info!("WhatsApp: pairing successful");
                            }
                            Event::Message(msg, info) => {
                                handler::handle_message(
                                    *msg,
                                    info,
                                    client,
                                    agent,
                                    session_svc,
                                    allowed,
                                    extra_sessions,
                                    voice_config,
                                    shared_session,
                                )
                                .await;
                            }
                            Event::LoggedOut(_) => {
                                tracing::warn!("WhatsApp: logged out");
                            }
                            Event::Disconnected(_) => {
                                tracing::warn!("WhatsApp: disconnected");
                            }
                            _ => {}
                        }
                    }
                })
                .build()
                .await;

            let mut bot = match bot_result {
                Ok(b) => b,
                Err(e) => {
                    tracing::error!("WhatsApp: failed to build bot: {}", e);
                    return;
                }
            };

            match bot.run().await {
                Ok(handle) => {
                    if let Err(e) = handle.await {
                        tracing::error!("WhatsApp agent task error: {:?}", e);
                    }
                }
                Err(e) => {
                    tracing::error!("WhatsApp agent error: {}", e);
                }
            }
        })
    }
}
