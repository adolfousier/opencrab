//! WhatsApp Integration
//!
//! Runs a WhatsApp Web client alongside the TUI, forwarding messages from
//! allowlisted phone numbers to the AgentService and replying with responses.

mod agent;
mod approval;
mod cancel;
mod connection;
mod followups;
pub(crate) mod handler;
mod onboarding_events;
mod pairing;
mod photos;
pub(crate) mod resume;
pub(crate) mod store;

pub use agent::WhatsAppAgent;
pub use approval::WaApproval;

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use whatsapp_rust::client::Client;

/// Shared WhatsApp client state for proactive messaging.
///
/// Set when the bot connects (either via static agent or whatsapp_connect tool).
/// Read by the `whatsapp_send` tool to send messages on demand.
pub struct WhatsAppState {
    client: Mutex<Option<Arc<Client>>>,
    /// Owner's JID (phone@s.whatsapp.net) — first in allowed_phones list
    owner_jid: Mutex<Option<String>>,
    /// Pending tool approvals: phone → oneshot sender of WaApproval.
    /// When a tool approval is in flight, the next message from that phone
    /// (text or button tap) is interpreted as Yes/Always/No instead of
    /// being routed to the agent.
    pub pending_approvals: Mutex<HashMap<String, tokio::sync::oneshot::Sender<WaApproval>>>,
    /// Pending follow-up questions keyed by phone: oneshot sender for
    /// the chosen option string plus the option list. WhatsApp's
    /// ButtonsMessage is deprecated, so we render the question as a
    /// numbered text list and parse the user's next numeric reply.
    /// Per-session OPTIONAL follow-up suggestions from `suggest_options`
    /// (#600). WhatsApp has no working button UI, so these render as a numbered
    /// text list; a bare numeric reply selects the matching suggestion. Keyed by
    /// session; consumed on a valid numeric reply, cleared on any other message.
    pending_followups: Mutex<HashMap<Uuid, Vec<String>>>,
    /// Per-session cancel tokens for aborting in-flight agent tasks via /stop
    cancel_tokens: Mutex<HashMap<Uuid, CancellationToken>>,
    /// Broadcast channel for QR codes — onboarding subscribes to this.
    qr_tx: tokio::sync::broadcast::Sender<String>,
    /// Broadcast channel for connection events — onboarding subscribes to this.
    connected_tx: tokio::sync::broadcast::Sender<()>,
    /// Broadcast channel for error events — onboarding subscribes to this.
    error_tx: tokio::sync::broadcast::Sender<String>,
    /// Broadcast channel for delivered message ids (from `ReceiptType::Delivered`
    /// receipts). The onboarding connection test waits on this so it confirms
    /// only when a message actually reached WhatsApp — not merely when the send
    /// stanza was transmitted (which still returns Ok even if the server later
    /// rejects it with error 400).
    delivered_tx: tokio::sync::broadcast::Sender<String>,
    /// Last QR code broadcast. The QR channel is a plain broadcast with no
    /// replay, so a connect flow that subscribes AFTER the agent already
    /// emitted its QR would see nothing until the next ~20s refresh (the
    /// "press Enter twice" bug). New subscribers replay this immediately.
    last_qr: std::sync::Mutex<Option<String>>,
    /// Set by the onboarding connect/reset flow to force a fresh pairing.
    /// `reconcile_whatsapp` aborts the live agent and starts a new one against
    /// the wiped `session.db`, so old auth is dropped at RUNTIME (not only on
    /// disk) and the agent re-pairs with a fresh QR.
    restart_requested: std::sync::atomic::AtomicBool,
    /// True once pairing/connection succeeds. Locks the QR: once connected, a
    /// late or stale `broadcast_qr` is suppressed so the onboarding UI never
    /// flashes a QR after the account is already linked. Reset by
    /// `request_restart` when a fresh pairing is requested.
    connected: std::sync::atomic::AtomicBool,
    /// Set on `Event::PairSuccess` so the subsequent `Event::Connected`
    /// knows this is a fresh pairing (first-time or re-pair after reset),
    /// not a routine reconnect after a restart. Consumed by
    /// `take_first_pair_pending` so the greeting fires only once per
    /// pairing and never on a plain app restart.
    first_pair_pending: std::sync::atomic::AtomicBool,
    /// Photo batching buffer: (chat_jid) → Vec<(img_marker, caption)>
    /// When multiple photos arrive in quick succession (WhatsApp sends
    /// each as a separate message), we buffer them and dispatch together.
    #[allow(clippy::type_complexity)]
    photo_buffer: Mutex<HashMap<String, Vec<(String, Option<String>)>>>,
    /// Photo debounce cancellation tokens: chat_jid → CancellationToken
    pub(crate) photo_debounce: Mutex<HashMap<String, CancellationToken>>,
    /// session_id → chat JID, so a finished background task can resume the
    /// originating chat (#731). WhatsApp keeps no other session→target map;
    /// registered on each handled turn.
    session_jids: Mutex<HashMap<Uuid, String>>,
}

impl Default for WhatsAppState {
    fn default() -> Self {
        Self::new()
    }
}

impl WhatsAppState {
    pub fn new() -> Self {
        let (qr_tx, _) = tokio::sync::broadcast::channel(8);
        let (connected_tx, _) = tokio::sync::broadcast::channel(4);
        let (error_tx, _) = tokio::sync::broadcast::channel(4);
        let (delivered_tx, _) = tokio::sync::broadcast::channel(32);
        Self {
            client: Mutex::new(None),
            owner_jid: Mutex::new(None),
            pending_approvals: Mutex::new(HashMap::new()),
            pending_followups: Mutex::new(HashMap::new()),
            cancel_tokens: Mutex::new(HashMap::new()),
            qr_tx,
            connected_tx,
            error_tx,
            delivered_tx,
            last_qr: std::sync::Mutex::new(None),
            restart_requested: std::sync::atomic::AtomicBool::new(false),
            connected: std::sync::atomic::AtomicBool::new(false),
            first_pair_pending: std::sync::atomic::AtomicBool::new(false),
            photo_buffer: Mutex::new(HashMap::new()),
            photo_debounce: Mutex::new(HashMap::new()),
            session_jids: Mutex::new(HashMap::new()),
        }
    }

    /// Map a session to the chat JID it is being handled in, so a finished
    /// background task can resume that chat (#731). Called on each turn.
    pub async fn register_session_jid(&self, session_id: Uuid, jid: String) {
        self.session_jids.lock().await.insert(session_id, jid);
    }

    /// The chat JID a session was last handled in, if known (#731).
    pub async fn session_jid(&self, session_id: Uuid) -> Option<String> {
        self.session_jids.lock().await.get(&session_id).cloned()
    }
}
