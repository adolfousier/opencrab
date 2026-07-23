//! WhatsApp Integration
//!
//! Runs a WhatsApp Web client alongside the TUI, forwarding messages from
//! allowlisted phone numbers to the AgentService and replying with responses.

mod agent;
pub(crate) mod follow_up_question;
pub(crate) mod handler;
pub(crate) mod resume;
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
    /// Per-session OPTIONAL follow-up suggestions from `suggest_followups`
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
            pending_questions: Mutex::new(HashMap::new()),
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

    /// Stash this session's optional follow-up suggestions (#600).
    pub async fn set_pending_followups(&self, session_id: Uuid, options: Vec<String>) {
        self.pending_followups
            .lock()
            .await
            .insert(session_id, options);
    }

    /// If this session has pending suggestions and `reply` parses as a 1-based
    /// option number in range, consume the whole set and return the chosen
    /// suggestion. Returns None otherwise (leaving the set for the caller to
    /// clear on a non-selecting message).
    pub async fn take_followup_by_reply(&self, session_id: Uuid, reply: &str) -> Option<String> {
        let parsed: usize = reply.trim().parse().ok()?;
        if parsed == 0 {
            return None;
        }
        let mut map = self.pending_followups.lock().await;
        let options = map.get(&session_id)?;
        let chosen = options.get(parsed - 1).cloned();
        if chosen.is_some() {
            map.remove(&session_id);
        }
        chosen
    }

    /// Drop this session's pending follow-up suggestions (non-selecting message).
    pub async fn clear_pending_followups(&self, session_id: Uuid) {
        self.pending_followups.lock().await.remove(&session_id);
    }

    /// Check whether a phone has a pending question without consuming
    /// it. Used by the message router to decide if the incoming text
    /// should be parsed as an answer rather than forwarded to the agent.
    pub async fn has_pending_question(&self, phone: &str) -> bool {
        self.pending_questions.lock().await.contains_key(phone)
    }

    /// Broadcast a QR code to any subscribed onboarding UI, and remember it so
    /// a subscriber that joins after this point can replay it immediately.
    ///
    /// No-op once connected: after pairing succeeds the QR is locked, so a late
    /// or stale QR event can never reappear in the onboarding UI.
    pub fn broadcast_qr(&self, code: &str) {
        if self.connected.load(std::sync::atomic::Ordering::SeqCst) {
            tracing::debug!("WhatsApp: suppressing QR broadcast — already connected");
            return;
        }
        *self.last_qr.lock().unwrap_or_else(|e| e.into_inner()) = Some(code.to_string());
        let _ = self.qr_tx.send(code.to_string());
    }

    /// The most recently broadcast QR, if any. Replayed to a new subscriber so
    /// it does not have to wait for the next refresh (fixes the "Enter twice"
    /// race). Cleared on [`request_restart`] and on connect.
    pub fn current_qr(&self) -> Option<String> {
        self.last_qr
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Request a fresh pairing: the next `reconcile_whatsapp` aborts the live
    /// agent and starts a new one against the wiped session. Clears the stored
    /// QR so the stale one is never replayed, and clears the connected flag so
    /// a new QR can be broadcast again for the re-pairing.
    pub fn request_restart(&self) {
        *self.last_qr.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.connected
            .store(false, std::sync::atomic::Ordering::SeqCst);
        self.restart_requested
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Consume the restart request (returns whether one was pending).
    pub fn take_restart_request(&self) -> bool {
        self.restart_requested
            .swap(false, std::sync::atomic::Ordering::SeqCst)
    }

    /// Mark that a fresh pairing just succeeded. The next `Connected` event
    /// will fire the one-time connection greeting.
    pub fn set_first_pair_pending(&self) {
        self.first_pair_pending
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Consume the first-pair flag. Returns `true` exactly once per pairing
    /// (first-time or re-pair after reset), then resets to `false` so a plain
    /// app restart never re-triggers the greeting.
    pub fn take_first_pair_pending(&self) -> bool {
        self.first_pair_pending
            .swap(false, std::sync::atomic::Ordering::SeqCst)
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

    /// Announce that a sent message id received a `Delivered` receipt. Called
    /// from the agent event loop so the onboarding test can confirm real
    /// delivery rather than mere transmission.
    pub fn broadcast_delivered(&self, message_id: &str) {
        let _ = self.delivered_tx.send(message_id.to_string());
    }

    /// Subscribe to delivered-message ids (used by the connection test).
    pub fn subscribe_delivered(&self) -> tokio::sync::broadcast::Receiver<String> {
        self.delivered_tx.subscribe()
    }

    /// Store the connected client and owner JID, then mark connected (which
    /// locks the QR) and notify onboarding subscribers.
    pub async fn set_connected(&self, client: Arc<Client>, owner_jid: Option<String>) {
        *self.client.lock().await = Some(client);
        if let Some(jid) = owner_jid {
            *self.owner_jid.lock().await = Some(jid);
        }
        self.mark_connected();
        self.broadcast_connected();
    }

    /// Flip the connected flag and drop any stale QR so it can never be
    /// replayed after pairing. Shared core of [`set_connected`]; exposed
    /// separately because unit tests cannot construct a live `Client`.
    pub fn mark_connected(&self) {
        self.connected
            .store(true, std::sync::atomic::Ordering::SeqCst);
        *self.last_qr.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// Record the freshly-paired owner's JID (`<number>@s.whatsapp.net`).
    /// Called on `PairSuccess` so the subsequent `Connected` handler and the
    /// `whatsapp_send` tool address the right account even on a first pairing
    /// where the startup-derived owner was unknown.
    pub async fn set_owner_jid(&self, jid: String) {
        *self.owner_jid.lock().await = Some(jid);
    }

    /// Get a clone of the connected client, if any.
    pub async fn client(&self) -> Option<Arc<Client>> {
        self.client.lock().await.clone()
    }

    /// Get the owner's JID for proactive messaging.
    pub async fn owner_jid(&self) -> Option<String> {
        self.owner_jid.lock().await.clone()
    }

    /// Check if WhatsApp is currently connected (reflects the connected flag set
    /// on pairing/connect and cleared on `request_restart`).
    pub async fn is_connected(&self) -> bool {
        self.connected.load(std::sync::atomic::Ordering::SeqCst)
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
            let caption = entries
                .iter()
                .find_map(|(_, c)| c.as_ref().filter(|s| !s.trim().is_empty()).cloned());
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
