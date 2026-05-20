//! Telegram Bot Integration
//!
//! Runs a Telegram bot alongside the TUI, forwarding messages from
//! allowlisted users to the AgentService and replying with responses.

mod agent;
pub(crate) mod follow_up_question;
pub(crate) mod handler;

pub use agent::TelegramAgent;

use std::collections::HashMap;
use teloxide::prelude::Bot;
use tokio::sync::{Mutex, oneshot};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// One pending `follow_up_question`: the oneshot half that the
/// `follow_up_question` tool is awaiting, plus the option list the
/// click handler uses to translate the button-index callback data
/// back into the chosen option string.
type PendingQuestion = (oneshot::Sender<String>, Vec<String>);

/// Shared Telegram state for proactive messaging.
///
/// Set when the bot connects (agent stores Bot) and when the owner
/// sends their first message (handler stores chat_id).
/// Read by the `telegram_send` tool to send messages on demand.
pub struct TelegramState {
    bot: Mutex<Option<Bot>>,
    /// Chat ID of the owner's conversation — used as default for proactive sends
    owner_chat_id: Mutex<Option<i64>>,
    /// Bot's @username — set at startup via get_me(), used for @mention detection in groups
    bot_username: Mutex<Option<String>>,
    /// Maps session_id → Telegram chat_id for approval routing
    session_chats: Mutex<HashMap<Uuid, i64>>,
    /// Reverse map: chat_id → session_id (kept in sync with session_chats)
    chat_sessions: Mutex<HashMap<i64, Uuid>>,
    /// Pending approval channels: approval_id → oneshot sender of (approved, always).
    pending_approvals: Mutex<HashMap<String, oneshot::Sender<(bool, bool)>>>,
    /// Pending follow-up questions: question_id → (oneshot sender of
    /// the chosen option string, list of options keyed by index). The
    /// inline-keyboard callback data only carries the option index (to
    /// stay under Telegram's 64-byte callback-data limit), so the
    /// option list is stashed here for the click handler to resolve
    /// `idx -> option string` before sending it back to the suspended
    /// `follow_up_question` tool.
    pending_questions: Mutex<HashMap<String, PendingQuestion>>,
    /// Per-session cancel tokens for aborting in-flight agent tasks via /stop
    cancel_tokens: Mutex<HashMap<Uuid, CancellationToken>>,
}

impl Default for TelegramState {
    fn default() -> Self {
        Self::new()
    }
}

impl TelegramState {
    pub fn new() -> Self {
        Self {
            bot: Mutex::new(None),
            owner_chat_id: Mutex::new(None),
            bot_username: Mutex::new(None),
            session_chats: Mutex::new(HashMap::new()),
            chat_sessions: Mutex::new(HashMap::new()),
            pending_approvals: Mutex::new(HashMap::new()),
            pending_questions: Mutex::new(HashMap::new()),
            cancel_tokens: Mutex::new(HashMap::new()),
        }
    }

    /// Store the connected Bot instance.
    pub async fn set_bot(&self, bot: Bot) {
        *self.bot.lock().await = Some(bot);
    }

    /// Update the owner's chat ID (called on each owner message).
    pub async fn set_owner_chat_id(&self, chat_id: i64) {
        *self.owner_chat_id.lock().await = Some(chat_id);
    }

    /// Get a clone of the Bot, if connected.
    pub async fn bot(&self) -> Option<Bot> {
        self.bot.lock().await.clone()
    }

    /// Get the owner's chat ID for proactive messaging.
    pub async fn owner_chat_id(&self) -> Option<i64> {
        *self.owner_chat_id.lock().await
    }

    /// Store the bot's @username (set at startup via get_me).
    pub async fn set_bot_username(&self, username: String) {
        *self.bot_username.lock().await = Some(username);
    }

    /// Get the bot's @username for mention detection.
    pub async fn bot_username(&self) -> Option<String> {
        self.bot_username.lock().await.clone()
    }

    /// Check if Telegram is currently connected.
    pub async fn is_connected(&self) -> bool {
        self.bot.lock().await.is_some()
    }

    /// Record which chat_id corresponds to a given session (for approval routing).
    /// Also maintains a reverse map so callbacks can resolve session from chat.
    pub async fn register_session_chat(&self, session_id: Uuid, chat_id: i64) {
        self.session_chats.lock().await.insert(session_id, chat_id);
        self.chat_sessions.lock().await.insert(chat_id, session_id);
    }

    /// Look up the chat_id for a given session_id.
    pub async fn session_chat(&self, session_id: Uuid) -> Option<i64> {
        self.session_chats.lock().await.get(&session_id).copied()
    }

    /// Reverse lookup: find the session_id for a given chat_id.
    /// Used by callback handlers to resolve the correct session for the chat
    /// where a button was pressed (instead of using the shared TUI session).
    pub async fn chat_session(&self, chat_id: i64) -> Option<Uuid> {
        self.chat_sessions.lock().await.get(&chat_id).copied()
    }

    /// Register a pending approval channel by id.
    pub async fn register_pending_approval(&self, id: String, tx: oneshot::Sender<(bool, bool)>) {
        self.pending_approvals.lock().await.insert(id, tx);
    }

    /// Resolve a pending approval.
    /// `approved` — whether tool is allowed; `always` — auto-approve all future tools.
    /// Returns true if a pending approval existed.
    pub async fn resolve_pending_approval(&self, id: &str, approved: bool, always: bool) -> bool {
        if let Some(tx) = self.pending_approvals.lock().await.remove(id) {
            let _ = tx.send((approved, always));
            true
        } else {
            false
        }
    }

    /// Register a pending follow-up question by id. The click handler
    /// later calls `resolve_pending_question(id, idx)` to deliver the
    /// chosen option string from `options[idx]`.
    pub async fn register_pending_question(
        &self,
        id: String,
        tx: oneshot::Sender<String>,
        options: Vec<String>,
    ) {
        self.pending_questions
            .lock()
            .await
            .insert(id, (tx, options));
    }

    /// Resolve a pending follow-up question by option index. Returns
    /// the chosen option string if the question was found and the
    /// index is in range, otherwise None.
    pub async fn resolve_pending_question(&self, id: &str, idx: usize) -> Option<String> {
        let entry = self.pending_questions.lock().await.remove(id);
        let (tx, options) = entry?;
        let answer = options.get(idx)?.clone();
        let _ = tx.send(answer.clone());
        Some(answer)
    }

    /// Store a cancel token for a session (before starting agent call).
    /// If a token already exists for this session, cancel it first to abort the
    /// previous in-flight agent call — this prevents concurrent agent calls from
    /// piling up on the same session and becoming uncancellable.
    pub async fn store_cancel_token(&self, session_id: Uuid, token: CancellationToken) {
        let mut tokens = self.cancel_tokens.lock().await;
        if let Some(old) = tokens.remove(&session_id) {
            tracing::warn!(
                "Telegram: cancelling previous in-flight agent call for session {}",
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
    /// Only removes if the stored token is already cancelled — this prevents a
    /// finishing old call from accidentally removing a newer call's live token.
    pub async fn remove_cancel_token(&self, session_id: Uuid) {
        let mut tokens = self.cancel_tokens.lock().await;
        if let Some(token) = tokens.get(&session_id)
            && token.is_cancelled()
        {
            tokens.remove(&session_id);
        }
    }
}
