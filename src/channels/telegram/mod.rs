//! Telegram Bot Integration
//!
//! Runs a Telegram bot alongside the TUI, forwarding messages from
//! allowlisted users to the AgentService and replying with responses.

mod agent;
pub(crate) mod cowork;
pub(crate) mod flow;
pub(crate) mod follow_up_question;
pub(crate) mod handler;
pub(crate) mod intermediates;
pub(crate) mod keyboards;
pub(crate) mod markdown;
pub(crate) mod media;
pub(crate) mod raw_updates;
pub(crate) mod reaction_prompt;
pub(crate) mod rich;
pub(crate) mod rich_decode;
pub(crate) mod send;
pub(crate) mod session_resolve;

pub use agent::TelegramAgent;
pub(crate) use agent::register_bot_commands;
#[cfg(test)]
pub(crate) use agent::{sanitize_command_name, truncate_description};

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

/// Photo buffer entry: (img_marker, Optional caption)
type PhotoEntry = (String, Option<String>);

/// Photo buffer key: (chat_id, user_id, media_group_id)
type PhotoBufferKey = (i64, i64, String);
type DirBrowserKey = (i64, Option<i32>);
type DirBrowserState = (String, Option<String>);

/// Shared Telegram state for proactive messaging.
///
/// Set when the bot connects (agent stores Bot) and when the owner
/// sends their first message (handler stores chat_id).
/// Read by the `telegram_send` tool to send messages on demand.
pub struct TelegramState {
    bot: Mutex<Option<Bot>>,
    /// Chat ID of the owner's conversation — used as default for proactive sends
    owner_chat_id: Mutex<Option<i64>>,
    /// Cached `(full_name, username)` of the owner, captured when an owner
    /// message arrives. Used to flag non-owner senders whose display name or
    /// username mimics the owner (impersonation detection in group chats).
    owner_identity: Mutex<Option<(String, Option<String>)>>,
    /// Bot's @username — set at startup via get_me(), used for @mention detection in groups
    bot_username: Mutex<Option<String>>,
    /// Bot's numeric user ID — set at startup via get_me(), used to distinguish
    /// replies to THIS bot from replies to other bots in group chats.
    bot_user_id: Mutex<Option<i64>>,
    /// Maps session_id → Telegram chat_id for approval routing. Topic-agnostic:
    /// approval/question replies route back by `chat_id` plus the per-message
    /// `thread_id` captured at send time, so the topic does not belong here.
    session_chats: Mutex<HashMap<Uuid, i64>>,
    /// Reverse map: (chat_id, forum_topic_id) → session_id. The topic component
    /// is `Some` only for genuine forum-topic messages (#215); DMs, non-forum
    /// groups, and the General topic key on `(chat_id, None)`, preserving the
    /// pre-topic behaviour. Each forum topic therefore binds its own session.
    chat_sessions: Mutex<HashMap<(i64, Option<i32>), Uuid>>,
    session_topic: Mutex<HashMap<Uuid, Option<i32>>>,
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
    /// Photo batching buffer: (chat_id, user_id, media_group_id) → Vec<(img_marker, Option<caption>)>
    /// When user sends multiple photos in an album, we buffer them and only fire the agent
    /// after a quiet period (no new photos for 3s). Keyed by media_group_id to avoid merging
    /// unrelated photos sent within 3s of each other.
    photo_buffer: Mutex<HashMap<PhotoBufferKey, Vec<PhotoEntry>>>,
    /// Photo debounce tokens: (chat_id, user_id, media_group_id) → CancellationToken
    /// Each new photo in the same album cancels the previous timer and starts a new 3s one.
    photo_debounce: Mutex<HashMap<PhotoBufferKey, CancellationToken>>,
    /// Active /cowork conversations: user_id → CoworkState
    cowork_conversations: Mutex<HashMap<i64, cowork::CoworkState>>,
    /// Cowork session lookup: session_id → CoworkState (for startgroup detection)
    cowork_sessions: Mutex<HashMap<String, cowork::CoworkState>>,
    /// Active sender tracking for auto mention-only mode (#244).
    /// Maps chat_id → set of user_ids that have sent ≥1 message.
    /// Set never shrinks — once >1 sender is detected, the chat
    /// permanently switches to mention-only until manually reset.
    active_senders: Mutex<HashMap<i64, std::collections::HashSet<i64>>>,
    /// Set of chat_ids that are cowork groups (for auto-register on join)
    cowork_groups: tokio::sync::Mutex<std::collections::HashSet<i64>>,
    /// Directory browser state: chat_id → (current_path, filter).
    /// Used by /cd inline-keyboard callbacks to know which directory
    /// is being browsed without encoding full paths in callback data.
    dir_browsers: Mutex<HashMap<DirBrowserKey, DirBrowserState>>,
    /// Profile create flow state: chat_id → true when awaiting a profile name
    prof_create_states: Mutex<HashMap<i64, bool>>,
    /// Pending file-save JoinHandles keyed by chat_id. The spawned task that
    /// downloads incoming media to tmp registers its handle here so the
    /// downstream tmp-photo pickup can `drain + await` before scanning,
    /// eliminating the race between fire-and-forget saves and mention handling.
    pending_file_saves: Mutex<HashMap<i64, Vec<tokio::task::JoinHandle<()>>>>,
    /// Reactions that landed while a turn was already running, waiting to be
    /// injected into that turn's tool loop between rounds. Keyed by session_id,
    /// drained FIFO (#302 Stage 2). `std::sync::Mutex` (not tokio) so the drain
    /// callback and the RAII active-turn guard can touch it without awaiting.
    pending_reactions: std::sync::Mutex<
        HashMap<Uuid, std::collections::VecDeque<crate::brain::agent::QueuedUserMessage>>,
    >,
    /// Sessions with an agent turn currently in flight, so `handle_reaction` can
    /// tell mid-turn (enqueue for injection) from idle (fire a standalone turn).
    /// Maintained via [`ActiveTurnGuard`] so a crashed turn can't leave a
    /// session looking permanently busy.
    active_turns: std::sync::Mutex<std::collections::HashSet<Uuid>>,
    /// Highest incoming Telegram message id seen per chat (#451). Recorded for
    /// EVERY message reaching the handler, mention or not, so the streaming
    /// edit loop can tell when its open flow block has been buried by newer
    /// chatter (block message id < newest incoming id) and re-stick the block
    /// to the bottom. Telegram message ids are per-chat monotonic, so a plain
    /// max is a valid "is the block buried" test.
    chat_newest_msg_id: std::sync::Mutex<HashMap<i64, i32>>,
}

impl Default for TelegramState {
    fn default() -> Self {
        Self::new()
    }
}

/// RAII guard that keeps a session marked "turn active" for its lifetime and
/// clears the flag on drop (#302 Stage 2). Held for the whole span of a
/// `handle_message` turn so a reaction arriving mid-turn is enqueued for
/// injection rather than firing a second concurrent turn on the same session.
pub(crate) struct ActiveTurnGuard {
    state: std::sync::Arc<TelegramState>,
    session_id: Uuid,
}

impl Drop for ActiveTurnGuard {
    fn drop(&mut self) {
        if let Ok(mut set) = self.state.active_turns.lock() {
            set.remove(&self.session_id);
        }
    }
}

impl TelegramState {
    pub fn new() -> Self {
        Self {
            bot: Mutex::new(None),
            owner_chat_id: Mutex::new(None),
            owner_identity: Mutex::new(None),
            bot_username: Mutex::new(None),
            bot_user_id: Mutex::new(None),
            session_chats: Mutex::new(HashMap::new()),
            chat_sessions: Mutex::new(HashMap::new()),
            session_topic: Mutex::new(HashMap::new()),
            pending_approvals: Mutex::new(HashMap::new()),
            pending_questions: Mutex::new(HashMap::new()),
            cancel_tokens: Mutex::new(HashMap::new()),
            photo_buffer: Mutex::new(HashMap::new()),
            photo_debounce: Mutex::new(HashMap::new()),
            cowork_conversations: Mutex::new(HashMap::new()),
            cowork_sessions: Mutex::new(HashMap::new()),
            active_senders: Mutex::new(HashMap::new()),
            cowork_groups: tokio::sync::Mutex::new(std::collections::HashSet::new()),
            dir_browsers: Mutex::new(HashMap::new()),
            prof_create_states: Mutex::new(HashMap::new()),
            pending_file_saves: Mutex::new(HashMap::new()),
            pending_reactions: std::sync::Mutex::new(HashMap::new()),
            active_turns: std::sync::Mutex::new(std::collections::HashSet::new()),
            chat_newest_msg_id: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Record an incoming message id for a chat, keeping the per-chat maximum
    /// (#451). Called at the top of the handler for every message so burial
    /// detection sees non-mention chatter too.
    pub(crate) fn note_incoming_msg(&self, chat_id: i64, msg_id: i32) {
        let mut map = self
            .chat_newest_msg_id
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let entry = map.entry(chat_id).or_insert(msg_id);
        if msg_id > *entry {
            *entry = msg_id;
        }
    }

    /// Newest incoming message id seen in a chat, if any (#451). The streaming
    /// edit loop compares this against its open flow block's message id to
    /// decide whether the block was buried and should re-stick to the bottom.
    pub(crate) fn newest_incoming_msg_id(&self, chat_id: i64) -> Option<i32> {
        self.chat_newest_msg_id
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&chat_id)
            .copied()
    }

    /// Store the connected Bot instance.
    pub async fn set_bot(&self, bot: Bot) {
        *self.bot.lock().await = Some(bot);
    }

    /// Update the owner's chat ID (called on each owner message).
    pub async fn set_owner_chat_id(&self, chat_id: i64) {
        *self.owner_chat_id.lock().await = Some(chat_id);
    }

    /// Cache the owner's display identity (captured from an owner message) so
    /// later non-owner senders can be checked for impersonation.
    pub async fn set_owner_identity(&self, full_name: String, username: Option<String>) {
        *self.owner_identity.lock().await = Some((full_name, username));
    }

    pub async fn owner_identity(&self) -> Option<(String, Option<String>)> {
        self.owner_identity.lock().await.clone()
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

    /// Store the bot's numeric user ID (set at startup via get_me).
    pub async fn set_bot_user_id(&self, id: i64) {
        *self.bot_user_id.lock().await = Some(id);
    }

    /// Get the bot's @username for mention detection.
    pub async fn bot_username(&self) -> Option<String> {
        self.bot_username.lock().await.clone()
    }

    /// Get the bot's numeric user ID for reply-to-bot detection.
    pub async fn bot_user_id(&self) -> Option<i64> {
        *self.bot_user_id.lock().await
    }

    /// Check if Telegram is currently connected.
    pub async fn is_connected(&self) -> bool {
        self.bot.lock().await.is_some()
    }

    /// Record which chat_id corresponds to a given session (for approval routing).
    /// Also maintains a reverse map so callbacks can resolve session from chat.
    ///
    /// The reverse map keys on `(chat_id, topic_id)` so distinct forum topics in
    /// one supergroup bind distinct sessions (#215); pass `None` for DMs,
    /// non-forum groups, and the General topic. The forward `session_chats` map
    /// stays topic-agnostic (approval routing only needs the chat_id).
    pub async fn register_session_chat(
        &self,
        session_id: Uuid,
        chat_id: i64,
        topic_id: Option<i32>,
    ) {
        self.session_chats.lock().await.insert(session_id, chat_id);
        self.session_topic.lock().await.insert(session_id, topic_id);
        self.chat_sessions
            .lock()
            .await
            .insert((chat_id, topic_id), session_id);
    }

    /// Look up the chat_id for a given session_id.
    pub async fn session_chat(&self, session_id: Uuid) -> Option<i64> {
        self.session_chats.lock().await.get(&session_id).copied()
    }

    /// Look up the forum topic_id for a given session_id. Returns `Some(tid)`
    /// for forum-topic sessions, `None` for DMs / non-forum groups / General.
    /// Used by `follow_up_question` and `make_approval_callback` to route
    /// messages to the correct forum topic (#247, #249).
    pub async fn session_topic(&self, session_id: Uuid) -> Option<i32> {
        self.session_topic
            .lock()
            .await
            .get(&session_id)
            .copied()
            .flatten()
    }

    /// Reverse lookup: find the session_id for a given chat_id, scoped to the
    /// forum topic (#215). Used by callback handlers to resolve the correct
    /// session for the chat where a button was pressed (instead of using the
    /// shared TUI session). `(chat_id, None)` matches the base/General session;
    /// `(chat_id, Some(tid))` matches that topic's own session.
    pub async fn chat_session(&self, chat_id: i64, topic_id: Option<i32>) -> Option<Uuid> {
        self.chat_sessions
            .lock()
            .await
            .get(&(chat_id, topic_id))
            .copied()
    }

    /// Register a pending file-save JoinHandle for a chat. The spawned task
    /// that downloads incoming media calls this so the tmp-photo pickup can
    /// await completion before scanning for files.
    pub async fn push_pending_save(&self, chat_id: i64, handle: tokio::task::JoinHandle<()>) {
        self.pending_file_saves
            .lock()
            .await
            .entry(chat_id)
            .or_default()
            .push(handle);
    }

    /// Drain all pending file-save handles for a chat and await each one.
    /// Called just before tmp-photo pickup to eliminate the race between
    /// fire-and-forget downloads and mention-triggered file lookups.
    pub async fn drain_pending_saves(&self, chat_id: i64) {
        let handles = self
            .pending_file_saves
            .lock()
            .await
            .remove(&chat_id)
            .unwrap_or_default();
        for h in handles {
            if let Err(e) = h.await {
                tracing::warn!("Telegram: pending file-save task panicked: {e}");
            }
        }
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
            // A completed turn's token stays in the map (remove_cancel_token
            // keeps non-cancelled tokens), so most entries found here are
            // stale leftovers of turns that finished normally. Only a turn
            // that is still ACTIVE is a genuine mid-flight kill worth a
            // warning (#439: the stale-token warn made a routine next
            // message look like the running task had been cancelled).
            if self.is_turn_active(session_id) {
                tracing::warn!(
                    "Telegram: cancelling previous in-flight agent call for session {}",
                    session_id
                );
            } else {
                tracing::debug!(
                    "Telegram: clearing stale cancel token of a completed turn for session {}",
                    session_id
                );
            }
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

    /// Mark `session_id` as having a turn in flight, returning an RAII guard
    /// that clears the flag on drop — normal return, early return, panic, or
    /// cancellation — so a crashed turn never leaves a session looking busy
    /// (which would silently queue every future reaction forever). #302 Stage 2.
    ///
    /// Deliberately does NOT reuse `cancel_tokens`: `remove_cancel_token` keeps
    /// a completed (non-cancelled) token in the map until the next turn, so
    /// `cancel_tokens.contains_key` yields false positives for idle sessions.
    pub(crate) fn mark_turn_active(
        self: &std::sync::Arc<Self>,
        session_id: Uuid,
    ) -> ActiveTurnGuard {
        if let Ok(mut set) = self.active_turns.lock() {
            set.insert(session_id);
        }
        ActiveTurnGuard {
            state: self.clone(),
            session_id,
        }
    }

    /// True while a turn is in flight for `session_id`.
    pub(crate) fn is_turn_active(&self, session_id: Uuid) -> bool {
        self.active_turns
            .lock()
            .map(|s| s.contains(&session_id))
            .unwrap_or(false)
    }

    /// Enqueue a mid-turn reaction message for injection into the running loop.
    pub(crate) fn enqueue_reaction(
        &self,
        session_id: Uuid,
        msg: crate::brain::agent::QueuedUserMessage,
    ) {
        if let Ok(mut map) = self.pending_reactions.lock() {
            map.entry(session_id).or_default().push_back(msg);
        }
    }

    /// Pop the next queued reaction for `session_id` (FIFO), if any. Removes the
    /// per-session entry once its queue is empty so the map doesn't grow.
    pub(crate) fn drain_reaction(
        &self,
        session_id: Uuid,
    ) -> Option<crate::brain::agent::QueuedUserMessage> {
        let mut map = self.pending_reactions.lock().ok()?;
        let queue = map.get_mut(&session_id)?;
        let msg = queue.pop_front();
        if queue.is_empty() {
            map.remove(&session_id);
        }
        msg
    }

    /// A [`MessageQueueCallback`](crate::brain::agent::MessageQueueCallback) that
    /// drains this state's pending reactions, keyed per session. Wired into the
    /// Telegram `AgentService` so the tool loop injects a queued reaction between
    /// rounds (the same rail the TUI uses for follow-up messages).
    pub(crate) fn reaction_queue_callback(
        self: &std::sync::Arc<Self>,
    ) -> crate::brain::agent::MessageQueueCallback {
        let state = self.clone();
        std::sync::Arc::new(move |session_id: Uuid| {
            let state = state.clone();
            Box::pin(async move { state.drain_reaction(session_id) })
        })
    }

    /// Buffer a photo marker for batching. Returns the current buffer size.
    /// Photos are accumulated per (chat_id, user_id, media_group_id) until the debounce timer expires.
    /// Only called for album photos (media_group_id is Some).
    pub async fn buffer_photo(
        &self,
        chat_id: i64,
        user_id: i64,
        media_group_id: &str,
        img_marker: String,
        caption: Option<String>,
    ) -> usize {
        let key = (chat_id, user_id, media_group_id.to_string());
        let mut buffer = self.photo_buffer.lock().await;
        buffer
            .entry(key.clone())
            .or_default()
            .push((img_marker, caption));
        buffer.get(&key).map(|v| v.len()).unwrap_or(0)
    }

    /// Reset the photo debounce timer for a (chat_id, user_id, media_group_id).
    /// Cancels any existing timer and creates a new one.
    /// Returns a CancellationToken that will be cancelled if another photo arrives.
    /// Only called for album photos (media_group_id is Some).
    pub async fn reset_photo_debounce(
        &self,
        chat_id: i64,
        user_id: i64,
        media_group_id: &str,
    ) -> CancellationToken {
        let key = (chat_id, user_id, media_group_id.to_string());
        let token = CancellationToken::new();

        let mut debounce = self.photo_debounce.lock().await;
        if let Some(old) = debounce.remove(&key) {
            old.cancel();
        }
        debounce.insert(key, token.clone());

        token
    }

    /// Wait for the photo debounce period (3 seconds) or until cancelled.
    /// Returns true if the timer expired (no new photos), false if cancelled.
    pub async fn wait_photo_debounce(&self, token: CancellationToken) -> bool {
        tokio::select! {
            _ = token.cancelled() => false,
            _ = tokio::time::sleep(std::time::Duration::from_secs(3)) => true,
        }
    }

    /// Drain all buffered photos for a (chat_id, user_id, media_group_id).
    /// Returns the vector of (img_marker, caption) tuples, or empty if none buffered.
    /// Only called for album photos (media_group_id is Some).
    pub async fn drain_photo_buffer(
        &self,
        chat_id: i64,
        user_id: i64,
        media_group_id: &str,
    ) -> Vec<(String, Option<String>)> {
        let key = (chat_id, user_id, media_group_id.to_string());
        let mut buffer = self.photo_buffer.lock().await;
        buffer.remove(&key).unwrap_or_default()
    }

    /// Clean up the debounce token after processing.
    /// Only called for album photos (media_group_id is Some).
    pub async fn cleanup_photo_debounce(&self, chat_id: i64, user_id: i64, media_group_id: &str) {
        let key = (chat_id, user_id, media_group_id.to_string());
        self.photo_debounce.lock().await.remove(&key);
    }

    // ── Cowork state management ──────────────────────────────────────────

    /// Start a new /cowork conversation for a user.
    pub async fn start_cowork(&self, user_id: i64, chat_id: i64, session_id: String) {
        let state = cowork::CoworkState::new(user_id, chat_id, session_id.clone());
        self.cowork_sessions
            .lock()
            .await
            .insert(session_id, state.clone());
        self.cowork_conversations
            .lock()
            .await
            .insert(user_id, state);
    }

    /// Get the active /cowork state for a user (if any).
    pub async fn get_cowork_state(&self, user_id: i64) -> Option<cowork::CoworkState> {
        self.cowork_conversations
            .lock()
            .await
            .get(&user_id)
            .cloned()
    }

    /// Take (remove) a cowork state by session_id. Used when bot joins a group.
    pub async fn take_cowork_by_session(&self, session_id: &str) -> Option<cowork::CoworkState> {
        let state = self.cowork_sessions.lock().await.remove(session_id);
        if let Some(ref s) = state {
            self.cowork_conversations.lock().await.remove(&s.user_id);
        }
        state
    }

    /// Clear the cowork state for a user.
    pub async fn clear_cowork(&self, user_id: i64) {
        if let Some(state) = self.cowork_conversations.lock().await.remove(&user_id) {
            self.cowork_sessions.lock().await.remove(&state.session_id);
        }
    }

    /// Add a chat_id to the tracked cowork groups set.
    pub async fn add_cowork_group(&self, chat_id: i64) {
        self.cowork_groups.lock().await.insert(chat_id);
    }

    /// Check if a chat_id is a tracked cowork group.
    pub async fn is_cowork_group(&self, chat_id: i64) -> bool {
        self.cowork_groups.lock().await.contains(&chat_id)
    }

    // ── Active sender tracking (#244) ───────────────────────────────────

    /// Record a message sender for a chat. Returns the total number of
    /// unique senders after adding this one. Set never shrinks.
    pub async fn track_active_sender(&self, chat_id: i64, user_id: i64) -> usize {
        let mut map = self.active_senders.lock().await;
        let set = map.entry(chat_id).or_default();
        set.insert(user_id);
        set.len()
    }

    // ── Directory browser state ─────────────────────────────────────────

    /// Set the browsing path for a chat+topic (called on /cd and navigation).
    pub async fn set_dir_browser(
        &self,
        chat_id: i64,
        topic_id: Option<i32>,
        path: String,
        filter: Option<String>,
    ) {
        self.dir_browsers
            .lock()
            .await
            .insert((chat_id, topic_id), (path, filter));
    }

    /// Get the current browsing path and filter for a chat+topic.
    pub async fn get_dir_browser(
        &self,
        chat_id: i64,
        topic_id: Option<i32>,
    ) -> Option<(String, Option<String>)> {
        self.dir_browsers
            .lock()
            .await
            .get(&(chat_id, topic_id))
            .cloned()
    }

    /// Clear the directory browser state for a chat+topic (after confirming).
    pub async fn clear_dir_browser(&self, chat_id: i64, topic_id: Option<i32>) {
        self.dir_browsers.lock().await.remove(&(chat_id, topic_id));
    }

    /// Set the profile-create flow state for a chat.
    pub async fn set_prof_create(&self, chat_id: i64, active: bool) {
        if active {
            self.prof_create_states.lock().await.insert(chat_id, true);
        } else {
            self.prof_create_states.lock().await.remove(&chat_id);
        }
    }

    /// Check if a chat is in the profile-create flow.
    pub async fn is_prof_create(&self, chat_id: i64) -> bool {
        self.prof_create_states
            .lock()
            .await
            .get(&chat_id)
            .copied()
            .unwrap_or(false)
    }

    /// Clear the profile-create flow state.
    pub async fn clear_prof_create(&self, chat_id: i64) {
        self.prof_create_states.lock().await.remove(&chat_id);
    }
}
