//! WhatsApp Integration
//!
//! Runs a WhatsApp Web client alongside the TUI, forwarding messages from
//! allowlisted phone numbers to the AgentService and replying with responses.

mod agent;
pub(crate) mod follow_up_question;
pub(crate) mod handler;
pub(crate) mod store;

pub use agent::WhatsAppAgent;

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// One pending `follow_up_question` on WhatsApp: oneshot half + the
/// option list to translate the user's numeric reply back into the
/// chosen option string.
type PendingWhatsAppQuestion = (tokio::sync::oneshot::Sender<String>, Vec<String>);
use whatsapp_rust::client::Client;

/// Approval choices mirroring the TUI's Yes / Always (session) / YOLO (permanent) / No.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaApproval {
    /// Approve this tool call once.
    Yes,
    /// Approve this and all future tool calls for the rest of the session.
    Always,
    /// Approve permanently (survives restarts).
    Yolo,
    /// Deny this tool call.
    No,
}

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
    pub pending_questions: Mutex<HashMap<String, PendingWhatsAppQuestion>>,
    /// Per-session cancel tokens for aborting in-flight agent tasks via /stop
    cancel_tokens: Mutex<HashMap<Uuid, CancellationToken>>,
    /// Broadcast channel for QR codes — onboarding subscribes to this.
    qr_tx: tokio::sync::broadcast::Sender<String>,
    /// Broadcast channel for connection events — onboarding subscribes to this.
    connected_tx: tokio::sync::broadcast::Sender<()>,
    /// Broadcast channel for error events — onboarding subscribes to this.
    error_tx: tokio::sync::broadcast::Sender<String>,
    /// Photo batching buffer: (chat_jid) → Vec<(img_marker, caption)>
    /// When multiple photos arrive in quick succession (WhatsApp sends
    /// each as a separate message), we buffer them and dispatch together.
    photo_buffer: Mutex<HashMap<String, Vec<(String, Option<String>)>>>,
    /// Photo debounce cancellation tokens: chat_jid → CancellationToken
    pub(crate) photo_debounce: Mutex<HashMap<String, CancellationToken>>,
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
        Self {
            client: Mutex::new(None),
            owner_jid: Mutex::new(None),
            pending_approvals: Mutex::new(HashMap::new()),
            pending_questions: Mutex::new(HashMap::new()),
            cancel_tokens: Mutex::new(HashMap::new()),
            qr_tx,
            connected_tx,
            error_tx,
            photo_buffer: Mutex::new(HashMap::new()),
            photo_debounce: Mutex::new(HashMap::new()),
        }
    }

    /// Register a pending approval for a phone number.
    pub async fn register_pending_approval(
        &self,
        phone: String,
        tx: tokio::sync::oneshot::Sender<WaApproval>,
    ) {
        self.pending_approvals.lock().await.insert(phone, tx);
    }

    /// Resolve a pending approval (called when user replies or taps a button).
    /// Returns `Some(choice)` if there was a pending approval, `None` otherwise.
    pub async fn resolve_pending_approval(
        &self,
        phone: &str,
        choice: WaApproval,
    ) -> Option<WaApproval> {
        if let Some(tx) = self.pending_approvals.lock().await.remove(phone) {
            let _ = tx.send(choice);
            Some(choice)
        } else {
            None
        }
    }

    /// Register a pending follow-up question for a phone number.
    pub async fn register_pending_question(
        &self,
        phone: String,
        tx: tokio::sync::oneshot::Sender<String>,
        options: Vec<String>,
    ) {
        self.pending_questions
            .lock()
            .await
            .insert(phone, (tx, options));
    }

    /// Resolve a pending question by parsing the user's text reply as
    /// a 1-based option number. Returns the chosen option if the phone
    /// had a pending question and the index is in range.
    pub async fn resolve_pending_question(&self, phone: &str, reply: &str) -> Option<String> {
        let parsed: usize = reply.trim().parse().ok()?;
        if parsed == 0 {
            return None;
        }
        let idx = parsed - 1;
        let (tx, options) = self.pending_questions.lock().await.remove(phone)?;
        let answer = options.get(idx)?.clone();
        let _ = tx.send(answer.clone());
        Some(answer)
    }

    /// Check whether a phone has a pending question without consuming
    /// it. Used by the message router to decide if the incoming text
    /// should be parsed as an answer rather than forwarded to the agent.
    pub async fn has_pending_question(&self, phone: &str) -> bool {
        self.pending_questions.lock().await.contains_key(phone)
    }

    /// Broadcast a QR code to any subscribed onboarding UI.
    pub fn broadcast_qr(&self, code: &str) {
        let _ = self.qr_tx.send(code.to_string());
    }

    /// Broadcast a connected event to any subscribed onboarding UI.
    pub fn broadcast_connected(&self) {
        let _ = self.connected_tx.send(());
    }

    /// Subscribe to QR code events (used by onboarding).
    pub fn subscribe_qr(&self) -> tokio::sync::broadcast::Receiver<String> {
        self.qr_tx.subscribe()
    }

    /// Subscribe to connection events (used by onboarding).
    pub fn subscribe_connected(&self) -> tokio::sync::broadcast::Receiver<()> {
        self.connected_tx.subscribe()
    }

    /// Broadcast an error to any subscribed onboarding UI.
    pub fn broadcast_error(&self, msg: &str) {
        let _ = self.error_tx.send(msg.to_string());
    }

    /// Subscribe to error events (used by onboarding).
    pub fn subscribe_error(&self) -> tokio::sync::broadcast::Receiver<String> {
        self.error_tx.subscribe()
    }

    /// Store the connected client and owner JID.
    pub async fn set_connected(&self, client: Arc<Client>, owner_jid: Option<String>) {
        *self.client.lock().await = Some(client);
        if let Some(jid) = owner_jid {
            *self.owner_jid.lock().await = Some(jid);
        }
        self.broadcast_connected();
    }

    /// Get a clone of the connected client, if any.
    pub async fn client(&self) -> Option<Arc<Client>> {
        self.client.lock().await.clone()
    }

    /// Get the owner's JID for proactive messaging.
    pub async fn owner_jid(&self) -> Option<String> {
        self.owner_jid.lock().await.clone()
    }

    /// Check if WhatsApp is currently connected.
    pub async fn is_connected(&self) -> bool {
        self.client.lock().await.is_some()
    }

    /// Store a cancel token for a session (before starting agent call).
    /// If a token already exists for this session, cancel it first to abort the
    /// previous in-flight agent call — prevents concurrent uncancellable agents.
    pub async fn store_cancel_token(&self, session_id: Uuid, token: CancellationToken) {
        let mut tokens = self.cancel_tokens.lock().await;
        if let Some(old) = tokens.remove(&session_id) {
            tracing::warn!(
                "WhatsApp: cancelling previous in-flight agent call for session {}",
                session_id
            );
            old.cancel();
        }
        tokens.insert(session_id, token);
    }

    /// Cancel and remove the token for a session. Returns true if a token existed.
    pub async fn cancel_session(&self, session_id: Uuid) -> bool {
        if let Some(token) = self.cancel_tokens.lock().await.remove(&session_id) {
            token.cancel();
            true
        } else {
            false
        }
    }

    /// Remove the cancel token after the agent call completes (cleanup).
    /// Only removes if the stored token is already cancelled — prevents a
    /// finishing old call from removing a newer call's live token.
    pub async fn remove_cancel_token(&self, session_id: Uuid) {
        let mut tokens = self.cancel_tokens.lock().await;
        if let Some(token) = tokens.get(&session_id)
            && token.is_cancelled()
        {
            tokens.remove(&session_id);
        }
    }

    /// Buffer a photo marker for batching. Returns the current buffer size.
    pub async fn buffer_photo(
        &self,
        chat_jid: &str,
        img_marker: String,
        caption: Option<String>,
    ) -> usize {
        let mut buffer = self.photo_buffer.lock().await;
        let entry = buffer.entry(chat_jid.to_string()).or_default();
        entry.push((img_marker, caption));
        entry.len()
    }

    /// Drain all buffered photos for a chat. Returns the markers and the
    /// first non-empty caption found (WhatsApp only captions the first image).
    pub async fn drain_photo_buffer(&self, chat_jid: &str) -> (Vec<String>, Option<String>) {
        let mut buffer = self.photo_buffer.lock().await;
        if let Some(entries) = buffer.remove(chat_jid) {
            let caption = entries.iter().find_map(|(_, c)| {
                c.as_ref().filter(|s| !s.trim().is_empty()).cloned()
            });
            let markers: Vec<String> = entries.into_iter().map(|(m, _)| m).collect();
            (markers, caption)
        } else {
            (Vec::new(), None)
        }
    }

    /// Reset the photo debounce timer for a chat. Returns a new CancellationToken
    /// that will be cancelled if another photo arrives before it expires.
    pub async fn reset_photo_debounce(&self, chat_jid: &str) -> CancellationToken {
        let mut debounce = self.photo_debounce.lock().await;
        if let Some(old_token) = debounce.remove(chat_jid) {
            old_token.cancel();
        }
        let token = CancellationToken::new();
        debounce.insert(chat_jid.to_string(), token.clone());
        token
    }

    /// Wait for the photo debounce to expire. Returns true if the timer expired
    /// (this task should process the buffer), false if cancelled (another photo
    /// arrived and will handle it).
    pub async fn wait_photo_debounce(&self, token: &CancellationToken) -> bool {
        tokio::select! {
            _ = token.cancelled() => false,
            _ = tokio::time::sleep(std::time::Duration::from_secs(3)) => true,
        }
    }

    /// Clean up the debounce token after processing.
    pub async fn cleanup_photo_debounce(&self, chat_jid: &str) {
        let mut debounce = self.photo_debounce.lock().await;
        debounce.remove(chat_jid);
    }
}
