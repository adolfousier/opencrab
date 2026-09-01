//! Shared Telegram runtime state.
//!
//! `TelegramState` and its turn-guard / cancel-token / queue / photo-buffer
//! plumbing, plus the small type aliases it uses. Split out of `mod.rs` so the
//! module root is declarations and re-exports only.

use super::cowork;
use std::collections::{HashMap, VecDeque};
use teloxide::prelude::Bot;
use teloxide::types::MessageId;
use tokio::sync::{Mutex, oneshot};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// The answer bubble a merged suggestion keyboard rides on.
///
/// When `suggest_options` attaches its buttons to the final response bubble
/// instead of posting a separate "Suggested next" message, the tap handler
/// needs the bubble's exact HTML to record the pick WITHOUT erasing the
/// answer text (`edit_message_text` replaces the whole body).
/// #59: bound on rescued dead-host records (FIFO eviction).
const STALE_HOST_CAP: usize = 32;

#[derive(Clone)]
pub(crate) struct MergedHost {
    pub message_id: MessageId,
    /// HTML last rendered in that bubble (final response text, plus the
    /// folded option list when present).
    pub html: String,
    /// Host lives on the native rich API: tap-record edits must ride
    /// `super::rich::api::edit_rich_html`, not teloxide's edit_message_text.
    pub rich: bool,
    /// #55: the keyboard was GLUED onto this bubble (glue tier — the body
    /// was not merge-safe, e.g. a table-bearing rich answer). The body is
    /// unknown/unsafe to rewrite, so a tap strips the keyboard markup-only
    /// and echoes the pick record as its own note instead of editing text.
    pub glued: bool,
}

/// Merge candidate captured by deliver_final_response (#tg-suggest-merge):
/// the bubble the final response landed in, whatever surface sent it.
#[derive(Clone)]
pub(crate) struct MergeBubble {
    pub message_id: MessageId,
    /// `Some` = merge-safe body (classic HTML or table-free rich markdown).
    /// `None` = rich answer carries a table (#55): merging would flatten it,
    /// but the id is still a valid GLUE target — `edit_message_reply_markup`
    /// attaches the keyboard without ever touching the body.
    pub body: Option<BubbleBody>,
}

/// How a captured [`MergeBubble`] was sent — decides which edit call merges
/// the suggestion controls onto it.
#[derive(Clone)]
pub(crate) enum BubbleBody {
    /// Classic parse-mode HTML exactly as delivered.
    Html(String),
    /// Native rich message sent from this markdown (tables render
    /// server-side). Merging re-renders via `rich::markdown_to_html`, so
    /// capture SKIPS table-bearing answers — rich HTML input flattens
    /// tables (#679) — and those keep the standalone fallback.
    Markdown(String),
}

/// One registered follow-up suggestion keyboard: which session it belongs to,
/// the options it offered, and — when the buttons were merged onto the answer
/// bubble (#tg-suggest-merge) — that bubble. Keyed by an opaque short token,
/// NOT by session: consecutive turns' keyboards coexist, and each tap
/// resolves against its OWN keyboard's set instead of whatever newer set
/// last overwrote a per-session slot (#1217).
#[derive(Clone)]
pub(crate) struct PendingFollowupEntry {
    pub session_id: Uuid,
    pub options: Vec<String>,
    pub host: Option<MergedHost>,
}

/// Photo buffer entry: (img_marker, Optional caption)
type PhotoEntry = (String, Option<String>);

/// Photo buffer key: (chat_id, user_id, media_group_id)
type PhotoBufferKey = (i64, i64, String);
type DirBrowserKey = (i64, Option<i32>);
type DirBrowserState = (String, Option<String>);

/// Where a mid-turn queued message came from (#1213).
///
/// The queue's designed consumer is the live tool loop, which drains it
/// between rounds and does not care about origin. The end-of-turn flush does:
/// it runs when the loop has already exited, and the two kinds need different
/// destinations from there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueuedOrigin {
    /// An emoji reaction. A single toolless round is the right size for it.
    Reaction,
    /// A background command or sub-agent result. Needs a real tool loop.
    DetachedWork,
}

/// A queued message with the origin that decides how it is flushed.
pub(crate) struct QueuedItem {
    pub(crate) origin: QueuedOrigin,
    pub(crate) msg: crate::brain::agent::QueuedUserMessage,
}

/// Sync mirror behind the channel-ownership gate (fork #17). Maintained ONLY
/// by `TelegramState::register_session_chat` — the single write site for the
/// ownership maps — so it cannot drift from them locally. See the
/// `channel_ownership` field doc for why a sync copy exists at all.
#[derive(Default)]
struct ChannelOwnershipMirror {
    /// session → the `(chat_id, topic_id)` it was bound to.
    session_channel: HashMap<Uuid, (i64, Option<i32>)>,
    /// `(chat_id, topic_id)` → the session currently bound to it.
    channel_owner: HashMap<(i64, Option<i32>), Uuid>,
}

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
    /// Sync mirror of the ownership triangle for the channel-ownership gate
    /// (fork #17): session → its `(chat, topic)` and `(chat, topic)` → current
    /// occupant. Written ONLY in `register_session_chat`, beside the async
    /// maps above — the delivery gate's probe is a sync closure and cannot
    /// take tokio mutexes. Never used for routing; the async maps stay the
    /// source of truth for everything else.
    channel_ownership: std::sync::Mutex<ChannelOwnershipMirror>,
    /// Evidence-based forum detection (#1220): chat_id → true once ANY
    /// thread-scoped message was seen from that chat. A bare thread id can
    /// only exist on forum topics (governor.rs reached the same rule), so
    /// one observation proves forum-ness permanently for the process
    /// lifetime. Consumed by `normalize_topic` at ingress to give the
    /// General topic its own session bucket instead of None.
    chat_forums: Mutex<HashMap<i64, bool>>,
    /// Pending approval channels: approval_id → oneshot sender of (approved, always).
    pending_approvals: Mutex<HashMap<String, oneshot::Sender<(bool, bool)>>>,
    /// Pending follow-up questions: question_id → (oneshot sender of
    /// the chosen option string, list of options keyed by index). The
    /// inline-keyboard callback data only carries the option index (to
    /// tool unblocks and returns it as the answer, instead of queueing.
    /// Per-session OPTIONAL follow-up suggestions surfaced by the
    /// `suggest_options` tool (#597). Unlike `pending_questions` these are
    /// non-blocking: the buttons ride under the response and a tap injects the
    /// chosen suggestion as the user's next message. Keyed by session so the
    /// tap handler resolves `idx -> suggestion string`; cleared on tap or when
    /// the user sends anything.
    pending_followups: Mutex<HashMap<String, PendingFollowupEntry>>,
    /// #59: merged-host records of stash entries cleared by #597 (user sent
    /// their own message). The stash entry dies, but RICH-merged buttons
    /// survive visually inside the bubble body — a stale-shell tap needs to
    /// know WHAT SHAPE of dead keyboard to strip and from where. Keyed
    /// token-first in a FIFO deque, bounded at STALE_HOST_CAP (oldest evicted).
    stale_hosts: Mutex<VecDeque<(String, MergedHost)>>,
    /// Solo-owner auto-registration cache (#1155): chat_id → decision already
    /// reached. `true` = eligible solo group, full owner catalog registered;
    /// `false` = evaluated and ineligible. Cleared on membership events so the
    /// next message re-evaluates instead of trusting a stale snapshot — the
    /// exact staleness class Gap 2 of #1155 describes.
    solo_evaluated: Mutex<HashMap<i64, bool>>,
    /// Per-session cancel tokens for aborting in-flight agent tasks via /stop
    cancel_tokens: Mutex<HashMap<Uuid, CancellationToken>>,
    /// Persistent plan-card message per session (#580): the single Telegram
    /// message that shows the plan title + checklist and the Approve/Discard
    /// keyboard, edited in place across the creation/execution/completion turns
    /// instead of re-rendered inside each per-turn flow block. The `String` is a
    /// content signature (rendered body + keyboard) so a per-tick refresh skips
    /// the edit API call when nothing changed — avoiding a redundant-edit
    /// rate-limit storm.
    plan_cards: Mutex<HashMap<Uuid, (teloxide::types::MessageId, String)>>,
    /// Durable backing for `plan_cards` (#809). The map alone is process-local,
    /// so a restart lost the tracked message id and the card could then neither
    /// be edited nor removed: the next turn posted a SECOND card below the
    /// stale one instead of updating it. Set once at startup; `None` on
    /// surfaces built without a database, which keeps the old in-memory-only
    /// behaviour rather than failing.
    plan_card_store: Mutex<Option<crate::db::repository::PlanCardRepository>>,
    /// Session → when card writes may resume, after Telegram asked us to wait
    /// (#814). Without this the next refresh wrote immediately and renewed the
    /// flood-control window, so the countdown never elapsed.
    plan_card_backoff: Mutex<HashMap<Uuid, std::time::Instant>>,
    /// Session → lock serialising card writes (#822).
    ///
    /// `refresh_plan_card` reads whether a card is tracked, decides to edit or
    /// post, then records the id. With nothing held across that, two concurrent
    /// refreshes both saw no card, both posted, and the second id overwrote the
    /// first: one card was left visible but untracked, so it could never be
    /// edited or deleted again. Concurrency here is routine, since the
    /// streaming path refreshes repeatedly while the settle and resume paths
    /// also fire.
    ///
    /// Per session, so unrelated chats never wait on each other.
    plan_card_locks: Mutex<HashMap<Uuid, std::sync::Arc<tokio::sync::Mutex<()>>>>,
    /// Session → when the card was last re-stuck (deleted and reposted at the
    /// bottom). The re-stick costs two writes, so it is worth doing
    /// occasionally to keep the card reachable and ruinous to do every settle.
    plan_card_restick: Mutex<HashMap<Uuid, std::time::Instant>>,
    /// Photo batching buffer: (chat_id, user_id, media_group_id) → Vec<(img_marker, Option<caption>)>
    /// When user sends multiple photos in an album, we buffer them and only fire the agent
    /// after a quiet period (no new photos for 3s). Keyed by media_group_id to avoid merging
    /// unrelated photos sent within 3s of each other.
    photo_buffer: Mutex<HashMap<PhotoBufferKey, Vec<PhotoEntry>>>,
    /// Photo debounce tokens: (chat_id, user_id, media_group_id) → CancellationToken
    /// Each new photo in the same album cancels the previous timer and starts a new 3s one.
    photo_debounce: Mutex<HashMap<PhotoBufferKey, CancellationToken>>,
    /// Fragments of a client-split long message: (chat_id, user_id) → texts, in
    /// arrival order.
    ///
    /// A message too long for one send is split by the Telegram client, and
    /// unlike an album the pieces carry NO grouping id — nothing marks fragment
    /// two as a continuation. Only near-limit messages are buffered, so an
    /// ordinary message never waits (#950).
    text_buffer: Mutex<HashMap<(i64, i64), Vec<String>>>,
    /// Debounce tokens for the above: each new fragment cancels the previous
    /// timer and starts a fresh one, so the window measures the gap between
    /// fragments rather than the age of the first.
    text_debounce: Mutex<HashMap<(i64, i64), CancellationToken>>,
    /// Active /cowork conversations: user_id → CoworkState
    cowork_conversations: Mutex<HashMap<i64, cowork::CoworkState>>,
    /// Cowork session lookup: session_id → CoworkState (for startgroup detection)
    cowork_sessions: Mutex<HashMap<String, cowork::CoworkState>>,
    /// Active sender tracking for auto mention-only mode (#244).
    /// Maps chat_id → set of user_ids that have sent ≥1 message.
    /// Set never shrinks — once >1 sender is detected, the chat
    /// permanently switches to mention-only until manually reset.
    active_senders: Mutex<HashMap<i64, std::collections::HashSet<i64>>>,
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
    pending_reactions: std::sync::Mutex<HashMap<Uuid, std::collections::VecDeque<QueuedItem>>>,
    /// Sessions with an agent turn currently in flight, so `handle_reaction` can
    /// tell mid-turn (enqueue for injection) from idle (fire a standalone turn).
    /// Maintained via [`ActiveTurnGuard`] so a crashed turn can't leave a
    /// session looking permanently busy.
    active_turns: std::sync::Mutex<std::collections::HashSet<Uuid>>,
    /// Newest NON-STICKY message id seen per chat (#451, semantics sharpened
    /// by #1150). This is burial EVIDENCE for the flow block: user messages
    /// (recorded in the handler) plus non-sticky bot bubbles — intermediate
    /// reports (#582), follow-up questions. Sticky elements deliberately do
    /// NOT feed it: flow-block reposts and plan-card reposts never call these
    /// recorders, so one restick can never manufacture evidence that buries
    /// the other sticky element (the ping-pong #1150 closes). Telegram message
    /// ids are per-chat monotonic, so a plain max is a valid "is the block
    /// buried" test.
    chat_newest_msg_id: std::sync::Mutex<HashMap<i64, i32>>,
    /// Per-chat instant of the last sticky-stack action (#1150): a flow-block
    /// restick or a plan-card move, each of which is a delete+create pair.
    /// One shared flood-control budget instead of two independent gates —
    /// uncoordinated bursts of both were exactly the #814 regression shape.
    last_sticky_action: std::sync::Mutex<HashMap<i64, std::time::Instant>>,
    /// Dedup of outbound media uploaded via `telegram_send` so an identical
    /// file+caption isn't delivered twice back-to-back (#721).
    media_dedup: super::outbound_dedup::MediaSendDedup,
    /// Tracks which session sent which inline-button callback data (#878).
    /// When `telegram_send send_buttons` posts buttons, each callback_data
    /// string is mapped to the sending session's UUID.  The callback
    /// dispatcher looks up this map BEFORE falling back to chat-context
    /// resolution, ensuring callbacks route to the session that ORIGINATED
    /// the buttons — not whatever session last handled a message in that
    /// chat.  Entries are ephemeral, in-memory, and evictable (LIFO
    /// overwrite is fine; one active set per session is the norm).
    callback_origins: std::sync::Mutex<HashMap<String, Uuid>>,
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
            channel_ownership: std::sync::Mutex::new(ChannelOwnershipMirror::default()),
            chat_forums: Mutex::new(HashMap::new()),
            pending_approvals: Mutex::new(HashMap::new()),
            pending_followups: Mutex::new(HashMap::new()),
            stale_hosts: Mutex::new(VecDeque::new()),
            solo_evaluated: Mutex::new(HashMap::new()),
            cancel_tokens: Mutex::new(HashMap::new()),
            plan_cards: Mutex::new(HashMap::new()),
            plan_card_store: Mutex::new(None),
            plan_card_backoff: Mutex::new(HashMap::new()),
            plan_card_locks: Mutex::new(HashMap::new()),
            plan_card_restick: Mutex::new(HashMap::new()),
            photo_buffer: Mutex::new(HashMap::new()),
            photo_debounce: Mutex::new(HashMap::new()),
            text_buffer: Mutex::new(HashMap::new()),
            text_debounce: Mutex::new(HashMap::new()),
            cowork_conversations: Mutex::new(HashMap::new()),
            cowork_sessions: Mutex::new(HashMap::new()),
            active_senders: Mutex::new(HashMap::new()),
            dir_browsers: Mutex::new(HashMap::new()),
            prof_create_states: Mutex::new(HashMap::new()),
            pending_file_saves: Mutex::new(HashMap::new()),
            pending_reactions: std::sync::Mutex::new(HashMap::new()),
            active_turns: std::sync::Mutex::new(std::collections::HashSet::new()),
            chat_newest_msg_id: std::sync::Mutex::new(HashMap::new()),
            last_sticky_action: std::sync::Mutex::new(HashMap::new()),
            media_dedup: super::outbound_dedup::MediaSendDedup::default(),
            callback_origins: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Claim an outbound media send, returning `true` if it is fresh and
    /// `false` if an identical file+caption was sent to this chat within the
    /// dedup window (#721). Callers skip the actual upload on `false`.
    pub(crate) fn claim_media_send(
        &self,
        action: &str,
        chat_id: i64,
        reference: &str,
        caption: Option<&str>,
    ) -> bool {
        let sig =
            super::outbound_dedup::MediaSendDedup::signature(action, chat_id, reference, caption);
        self.media_dedup.claim(sig, std::time::Instant::now())
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

    /// Record a NON-STICKY bot-sent bubble as burial evidence (#1150):
    /// intermediate rich reports (#582) and follow-up question bubbles land
    /// below the flow block exactly like user chatter, but before this
    /// recorder nothing tracked them, so the block stayed buried under its own
    /// output for the rest of the turn. Sticky sends (flow-block reposts,
    /// plan-card reposts) must NEVER call this — their ids are positions, not
    /// evidence, and counting them is the restick ping-pong #1150 closes.
    pub(crate) fn note_bot_bubble(&self, chat_id: i64, msg_id: i32) {
        self.note_incoming_msg(chat_id, msg_id);
    }

    /// Claim permission for one sticky-stack action (flow-block restick or
    /// plan-card move) in `chat_id` (#1150). Returns false when another such
    /// action fired less than `min_interval` ago; the caller then skips its
    /// delete+create pair instead of bursting toward Telegram's flood control
    /// (#814). Both sticky mechanisms draw from this one budget so they can't
    /// combine into the storm two independent gates allowed.
    pub(crate) fn claim_sticky_action(
        &self,
        chat_id: i64,
        min_interval: std::time::Duration,
    ) -> bool {
        let mut map = self
            .last_sticky_action
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let now = std::time::Instant::now();
        if let Some(last) = map.get(&chat_id)
            && now.duration_since(*last) < min_interval
        {
            return false;
        }
        map.insert(chat_id, now);
        true
    }

    /// Minimum spacing between ANY sticky-stack actions in one chat (#1150):
    /// a flow-block restick and its coordinated plan-card move count as ONE
    /// action; the next restick (either mechanism) waits this long. Bounds
    /// delete+create churn well under Telegram's group flood limits while
    /// keeping #451 burial recovery responsive.
    pub(crate) const STICKY_STACK_MIN_INTERVAL: std::time::Duration =
        std::time::Duration::from_secs(15);

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

    /// Register callback_data → originating session for inline buttons (#878).
    /// Called by `telegram_send send_buttons` so the callback dispatcher can
    /// route taps to the session that SENT the buttons, not the one bound
    /// to the chat.
    pub(crate) fn register_callback_origins(
        &self,
        session_id: Uuid,
        callback_datas: impl IntoIterator<Item = String>,
    ) {
        if let Ok(mut map) = self.callback_origins.lock() {
            for data in callback_datas {
                map.insert(data, session_id);
            }
        }
    }

    /// Look up which session originated a given callback_data string (#878).
    pub(crate) fn lookup_callback_origin(&self, callback_data: &str) -> Option<Uuid> {
        self.callback_origins
            .lock()
            .ok()
            .and_then(|map| map.get(callback_data).copied())
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

    /// Cached solo-owner evaluation for a chat, if one was reached (#1155).
    pub async fn solo_evaluated(&self, chat_id: i64) -> Option<bool> {
        self.solo_evaluated.lock().await.get(&chat_id).copied()
    }

    /// Record a solo-owner evaluation outcome (#1155).
    pub async fn set_solo_evaluated(&self, chat_id: i64, eligible: bool) {
        self.solo_evaluated.lock().await.insert(chat_id, eligible);
    }

    /// Forget a chat's solo-owner evaluation — membership changed, next
    /// message re-evaluates (#1155).
    pub async fn clear_solo_evaluated(&self, chat_id: i64) {
        self.solo_evaluated.lock().await.remove(&chat_id);
    }

    /// Check if Telegram is currently connected.
    pub async fn is_connected(&self) -> bool {
        self.bot.lock().await.is_some()
    }

    /// Record forum-ness evidence for a chat (#1220). Any message carrying a
    /// bare `thread_id` proves the chat is a forum group — ordinary
    /// reply-threads never set it (same rule governor.rs uses). One-way:
    /// forum-ness never un-proves itself for the process lifetime.
    pub async fn note_thread_evidence(&self, chat_id: i64, thread_id: Option<i32>) {
        if thread_id.is_some() {
            self.chat_forums.lock().await.insert(chat_id, true);
        }
    }

    /// Whether this chat has ever hosted a thread-scoped message (#1220).
    /// False until the first observation — the cold-start window during which
    /// General-topic messages keep the legacy shared-session behaviour.
    pub async fn is_known_forum(&self, chat_id: i64) -> bool {
        self.chat_forums
            .lock()
            .await
            .get(&chat_id)
            .copied()
            .unwrap_or(false)
    }

    /// Record which chat_id corresponds to a given session (for approval routing).
    /// Also maintains a reverse map so callbacks can resolve session from chat.
    ///
    /// The reverse map keys on `(chat_id, topic_id)` so distinct forum topics in
    /// one supergroup bind distinct sessions (#215); since #1220 the General
    /// topic of a KNOWN forum passes `Some(GENERAL_TOPIC_ID)` (normalized at
    /// ingress), while plain `None` remains for DMs / non-forum groups. The
    /// forward `session_chats` map stays topic-agnostic (approval routing only
    /// needs the chat_id).
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
        // Keep the sync ownership mirror in lockstep (fork #17). This is the
        // ONLY write site for the maps above, so one extra lock keeps the
        // mirror exact; the delivery gate's probe reads it synchronously.
        if let Ok(mut mirror) = self.channel_ownership.lock() {
            mirror
                .session_channel
                .insert(session_id, (chat_id, topic_id));
            mirror.channel_owner.insert((chat_id, topic_id), session_id);
        }
    }

    /// Look up the chat_id for a given session_id.
    pub async fn session_chat(&self, session_id: Uuid) -> Option<i64> {
        self.session_chats.lock().await.get(&session_id).copied()
    }

    /// Look up the forum topic_id for a given session_id. Returns `Some(tid)`
    /// for forum-topic sessions — including General-topic sessions of known
    /// forums (`Some(GENERAL_TOPIC_ID)`, #1220) — and `None` for DMs /
    /// non-forum groups. Used by `make_approval_callback` to route
    /// messages to the correct forum topic (#247, #249).
    pub async fn session_topic(&self, session_id: Uuid) -> Option<i32> {
        self.session_topic
            .lock()
            .await
            .get(&session_id)
            .copied()
            .flatten()
    }

    /// Does `session_id` still own the channel it was bound to (fork #17)?
    /// Reads the sync ownership mirror (see field doc): `Owned` when the bound
    /// chat/topic still resolves back to this session, `Occupied` naming the
    /// session that took it over, `Unknown` when no binding was ever recorded
    /// (fresh boot, or the session never claimed since one). Callers: the
    /// delivery gate's probe (session_routes.rs) and the bg-resume
    /// choke-point guard (resume.rs).
    pub(crate) fn channel_ownership_of(
        &self,
        session_id: Uuid,
    ) -> crate::brain::agent::service::session_routes::ChannelOwnership {
        use crate::brain::agent::service::session_routes::ChannelOwnership;
        let Ok(mirror) = self.channel_ownership.lock() else {
            return ChannelOwnership::Unknown;
        };
        let Some(channel) = mirror.session_channel.get(&session_id).copied() else {
            return ChannelOwnership::Unknown;
        };
        match mirror.channel_owner.get(&channel).copied() {
            Some(owner) if owner != session_id => ChannelOwnership::Occupied { occupant: owner },
            Some(_) => ChannelOwnership::Owned,
            None => ChannelOwnership::Unknown,
        }
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

    /// Stash this session's optional follow-up suggestions (#597) so the tap
    /// handler can resolve `idx -> suggestion string`. Replaces any prior set.
    /// `host` is set when the keyboard was MERGED onto the final response
    /// bubble: the tap handler uses it to record the pick without erasing
    /// the answer text.
    pub(crate) async fn register_pending_followups(
        &self,
        session_id: Uuid,
        options: Vec<String>,
    ) -> String {
        let token = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
        let count = options.len();
        self.pending_followups.lock().await.insert(
            token.clone(),
            PendingFollowupEntry {
                session_id,
                options,
                host: None,
            },
        );
        // #1226: stash lifecycle used to be invisible — a register line lets
        // every later tap be mapped to the arm that minted its token.
        tracing::info!(
            "Telegram followups: registered stash token {token} for session \
             {session_id} ({count} options)"
        );
        token
    }

    /// Record the answer-bubble host after a successful merge-edit (#1217).
    pub(crate) async fn attach_followup_host(&self, token: &str, host: MergedHost) {
        if let Some(entry) = self.pending_followups.lock().await.get_mut(token) {
            entry.host = Some(host);
        }
    }

    /// Peek at the merged host WITHOUT consuming the stash (#31): the
    /// trailer-send decision in render_suggestions needs to know whether the
    /// RICH merge actually landed (the embedded trailer shipped with it) or
    /// the placement fell back to standalone (no embed ever hit the wire).
    /// `attach_followup_host` fires only on merge success, so Some(host) ==
    /// "the merge path landed".
    pub(crate) async fn peek_followup_host(&self, token: &str) -> Option<MergedHost> {
        self.pending_followups
            .lock()
            .await
            .get(token)
            .and_then(|e| e.host.clone())
    }

    /// #59: peek the dead-host record of a #597-cleared stash entry WITHOUT
    /// consuming it — the stale-shell tap needs the host SHAPE (rich body
    /// buttons vs glued/classic reply-markup) to pick the right strip; the
    /// record is forgotten only after the strip succeeds (`forget_stale_host`).
    pub(crate) async fn peek_stale_host(&self, token: &str) -> Option<MergedHost> {
        self.stale_hosts
            .lock()
            .await
            .iter()
            .find(|(t, _)| t == token)
            .map(|(_, h)| h.clone())
    }

    /// #59: current size of the stale-host map (bounded diagnostics).
    pub(crate) async fn stale_host_count(&self) -> usize {
        self.stale_hosts.lock().await.len()
    }

    /// #59: forget a stale-host record once its dead keyboard is confirmed
    /// stripped — repeated taps on the same zombie stay log-attributable and
    /// cannot loop on a record whose buttons are already gone.
    pub(crate) async fn forget_stale_host(&self, token: &str) {
        let mut stale = self.stale_hosts.lock().await;
        let before = stale.len();
        stale.retain(|(t, _)| t != token);
        if stale.len() < before {
            tracing::info!("Telegram followups: forgot stripped stale-host record {token} (#59)");
        }
    }

    /// Forget an unused registration (buttons never landed).
    pub(crate) async fn drop_pending_followup(&self, token: &str) {
        let existed = self.pending_followups.lock().await.remove(token).is_some();
        if existed {
            tracing::info!("Telegram followups: dropped unplaced stash token {token}");
        }
    }

    /// Take a tapped follow-up suggestion by index, consuming the WHOLE set for
    /// the session (one suggestion starts one turn; the rest are stale). Returns
    /// the suggestion string plus the merged host (when the keyboard lived on
    /// the answer bubble), or None if nothing is pending or the index is out
    /// of range.
    pub(crate) async fn take_pending_followup(
        &self,
        token: &str,
        idx: usize,
    ) -> Option<(Uuid, String, Option<MergedHost>)> {
        let entry = self.pending_followups.lock().await.remove(token)?;
        let host = entry.host.clone();
        entry
            .options
            .get(idx)
            .cloned()
            .map(|text| (entry.session_id, text, host))
    }

    /// Drop this session's pending follow-up suggestions (the user sent their
    /// own message, so the buttons are stale).
    pub async fn clear_pending_followups(&self, session_id: Uuid) {
        // #59: before wiping, rescue the merged-host records into `stale_hosts`
        // — the #597 clear kills the stash, but rich-merged buttons keep
        // rendering inside the bubble body. Without the rescued shape the
        // stale-shell tap can only try a blind markup strip, which on a rich
        // host is a guaranteed "message is not modified" no-op (the zombie).
        let rescued: Vec<(String, MergedHost)> = {
            let mut map = self.pending_followups.lock().await;
            let mut rescued = Vec::new();
            map.retain(|token, e| {
                if e.session_id == session_id {
                    if let Some(h) = e.host.take() {
                        rescued.push((token.clone(), h));
                    }
                    false
                } else {
                    true
                }
            });
            rescued
        };
        let removed = rescued.len();
        if !rescued.is_empty() {
            let mut stale = self.stale_hosts.lock().await;
            for pair in rescued {
                stale.push_back(pair);
            }
            while stale.len() > STALE_HOST_CAP {
                if let Some((t, _)) = stale.pop_front() {
                    tracing::debug!(
                        "Telegram followups: evicted oldest stale-host record {t} (cap {STALE_HOST_CAP})"
                    );
                }
            }
        }
        if removed > 0 {
            // #1226: the #597 wipe used to leave zero log lines, which made
            // stale-shell taps impossible to explain from logs alone.
            tracing::info!(
                "Telegram followups: cleared {removed} stale stash entries for \
                 session {session_id} (#597 — user sent their own message)"
            );
        }
    }

    /// Store a cancel token for a session (before starting an agent call).
    ///
    /// Overwrites any prior token WITHOUT cancelling it (#652). The old code
    /// cancelled the previous token here "to prevent concurrent calls piling
    /// up", but that predates the atomic `try_begin_turn` gate (#501) which is
    /// now the single source of truth for one-turn-per-session. With that gate
    /// in place the cancel here is redundant and harmful: a near-simultaneous
    /// resend that reached this point would hard-cancel the user's genuinely
    /// in-flight turn, dropping the running request instead of queuing it
    /// (`is_turn_active` couldn't tell an old live turn apart from the new
    /// turn's own just-set guard, so it "cancelled in-flight" on routine
    /// resends). A completed turn leaves a stale, non-cancelled token
    /// (`remove_cancel_token` only reaps cancelled ones); overwriting it is
    /// enough. Genuine cancellation (/stop, provider swap) goes through
    /// `cancel_session`, not here.
    pub async fn store_cancel_token(&self, session_id: Uuid, token: CancellationToken) {
        self.cancel_tokens.lock().await.insert(session_id, token);
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

    /// The persistent plan-card message id + last rendered signature for a
    /// session, if one is live (#580).
    pub(crate) async fn plan_card(
        &self,
        session_id: Uuid,
    ) -> Option<(teloxide::types::MessageId, String)> {
        if let Some(hit) = self.plan_cards.lock().await.get(&session_id).cloned() {
            return Some(hit);
        }
        // Miss: either no card, or this process just started and the map is
        // empty. Rehydrating here rather than scanning every session at boot
        // means the cost is paid once, only for sessions that actually ask
        // (#809).
        let stored = {
            let guard = self.plan_card_store.lock().await;
            let repo = guard.as_ref()?;
            match repo.get(&session_id.to_string()).await {
                Ok(row) => row?,
                Err(e) => {
                    tracing::warn!("Plan-card lookup failed for session {session_id}: {e}");
                    return None;
                }
            }
        };
        let card = (
            teloxide::types::MessageId(stored.message_id as i32),
            stored.signature,
        );
        self.plan_cards
            .lock()
            .await
            .insert(session_id, card.clone());
        tracing::info!("Recovered plan card for session {session_id} after restart");
        Some(card)
    }

    /// The lock serialising card writes for a session (#822).
    ///
    /// Returned as an `Arc` so the caller holds it across its API calls; the
    /// inner map lock is released immediately, so acquiring one session's lock
    /// never blocks another's.
    pub(crate) async fn plan_card_lock(
        &self,
        session_id: Uuid,
    ) -> std::sync::Arc<tokio::sync::Mutex<()>> {
        self.plan_card_locks
            .lock()
            .await
            .entry(session_id)
            .or_insert_with(|| std::sync::Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    /// Are card writes for this session currently suppressed (#814)?
    pub(crate) async fn plan_card_suppressed(&self, session_id: Uuid) -> bool {
        let mut map = self.plan_card_backoff.lock().await;
        match map.get(&session_id) {
            Some(until) if *until > std::time::Instant::now() => true,
            Some(_) => {
                // Window elapsed; drop the entry so the map cannot grow
                // without bound across long-lived sessions.
                map.remove(&session_id);
                false
            }
            None => false,
        }
    }

    /// Suppress card writes for this session for `wait`.
    pub(crate) async fn suppress_plan_card(&self, session_id: Uuid, wait: std::time::Duration) {
        self.plan_card_backoff
            .lock()
            .await
            .insert(session_id, std::time::Instant::now() + wait);
    }

    /// Should the card be re-stuck (deleted and reposted at the bottom) now?
    ///
    /// Records the decision, so a `true` starts the cooldown. Re-sticking keeps
    /// the card from being buried by streamed output, but costs a delete plus a
    /// create; doing it every settle is what put the card within reach of flood
    /// control (#814, regression from 3c45e41a).
    pub(crate) async fn should_restick_plan_card(
        &self,
        session_id: Uuid,
        cooldown: std::time::Duration,
    ) -> bool {
        let now = std::time::Instant::now();
        let mut map = self.plan_card_restick.lock().await;
        match map.get(&session_id) {
            Some(last) if now.duration_since(*last) < cooldown => false,
            _ => {
                map.insert(session_id, now);
                true
            }
        }
    }

    /// Give the plan-card map durable backing. Called once at startup.
    pub(crate) async fn set_plan_card_store(
        &self,
        repo: crate::db::repository::PlanCardRepository,
    ) {
        *self.plan_card_store.lock().await = Some(repo);
    }

    /// Track the plan-card message id + its rendered signature for a session.
    pub(crate) async fn set_plan_card(
        &self,
        session_id: Uuid,
        chat_id: teloxide::types::ChatId,
        thread_id: Option<teloxide::types::ThreadId>,
        msg_id: teloxide::types::MessageId,
        signature: String,
    ) {
        self.plan_cards
            .lock()
            .await
            .insert(session_id, (msg_id, signature.clone()));
        // Persist alongside, so a restart can still find and update THIS
        // message instead of posting a second card below the stale one (#809).
        let guard = self.plan_card_store.lock().await;
        if let Some(repo) = guard.as_ref()
            && let Err(e) = repo
                .set(crate::db::repository::PlanCard {
                    session_id: session_id.to_string(),
                    chat_id: chat_id.0,
                    thread_id: thread_id.map(|t| t.0.0 as i64),
                    message_id: msg_id.0 as i64,
                    signature,
                })
                .await
        {
            tracing::warn!("Failed to persist plan card for session {session_id}: {e}");
        }
    }

    /// Stop tracking the plan card and return the id that was tracked, if any
    /// (the caller deletes the message). Used on discard / plan completion.
    pub(crate) async fn take_plan_card(
        &self,
        session_id: Uuid,
    ) -> Option<teloxide::types::MessageId> {
        // Resolve through `plan_card` so a card tracked by a PREVIOUS process
        // is still removable. Without that, a restart left the stale card
        // undeletable and it lingered in the chat forever (#809).
        let existing = self.plan_card(session_id).await.map(|(mid, _)| mid);
        self.plan_cards.lock().await.remove(&session_id);
        let guard = self.plan_card_store.lock().await;
        if let Some(repo) = guard.as_ref()
            && let Err(e) = repo.delete(&session_id.to_string()).await
        {
            tracing::warn!("Failed to clear plan card for session {session_id}: {e}");
        }
        existing
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

    /// Atomically begin a turn for `session_id` (#501). Under ONE lock:
    /// returns `Some(guard)` and marks the session active when no turn is
    /// running, or `None` when a turn already is (the caller then treats the
    /// message as a mid-turn follow-up). This closes the check-then-act race
    /// where `is_turn_active` was checked ~600 lines before `mark_turn_active`
    /// ran, so a follow-up arriving in that window read the session as idle
    /// and forked a second concurrent turn instead of enqueuing.
    pub(crate) fn try_begin_turn(
        self: &std::sync::Arc<Self>,
        session_id: Uuid,
    ) -> Option<ActiveTurnGuard> {
        let mut set = self.active_turns.lock().ok()?;
        if !set.insert(session_id) {
            // Already present: a turn is in flight for this session.
            return None;
        }
        Some(ActiveTurnGuard {
            state: self.clone(),
            session_id,
        })
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
        self.enqueue_item(session_id, QueuedOrigin::Reaction, msg);
    }

    /// Enqueue the result of DETACHED work — a background command or a
    /// sub-agent — for injection into the running loop.
    ///
    /// Same queue, different origin, because the two need different treatment
    /// if the loop ends before draining them (#1213). A reaction is an
    /// acknowledgement and a single toolless round answers it fine. A
    /// detached result is evidence the agent is expected to ACT on, and
    /// answering it without a tool registry produces a turn that announces
    /// follow-up work it is structurally unable to perform.
    pub(crate) fn enqueue_detached_result(
        &self,
        session_id: Uuid,
        msg: crate::brain::agent::QueuedUserMessage,
    ) {
        self.enqueue_item(session_id, QueuedOrigin::DetachedWork, msg);
    }

    fn enqueue_item(
        &self,
        session_id: Uuid,
        origin: QueuedOrigin,
        msg: crate::brain::agent::QueuedUserMessage,
    ) {
        match self.pending_reactions.lock() {
            Ok(mut map) => map
                .entry(session_id)
                .or_default()
                .push_back(QueuedItem { origin, msg }),
            Err(e) => {
                // The message is gone at this point, and for detached work
                // that means a result nobody will ever see.
                tracing::error!(
                    "Telegram: could not queue a {origin:?} message for session {session_id}, \
                     it is dropped: {e}"
                );
            }
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
        let item = queue.pop_front();
        if queue.is_empty() {
            map.remove(&session_id);
        }
        item.map(|i| i.msg)
    }

    /// Take everything still queued for `session_id`, WITH origins.
    ///
    /// Used by the end-of-turn flush, which is the one consumer that has to
    /// tell the two apart: by then the loop has exited, so whatever is left
    /// needs a destination chosen by what it is (#1213).
    pub(crate) fn drain_queued_items(&self, session_id: Uuid) -> Vec<QueuedItem> {
        match self.pending_reactions.lock() {
            Ok(mut map) => map.remove(&session_id).map(Vec::from).unwrap_or_default(),
            Err(e) => {
                tracing::error!(
                    "Telegram: could not drain queued messages for session {session_id}: {e}"
                );
                Vec::new()
            }
        }
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

    /// Buffer a fragment of a possibly-split message. Returns how many are held.
    pub async fn buffer_text(&self, chat_id: i64, user_id: i64, text: String) -> usize {
        let mut buffer = self.text_buffer.lock().await;
        let frags = buffer.entry((chat_id, user_id)).or_default();
        frags.push(text);
        frags.len()
    }

    /// Start (or restart) the wait for another fragment from this sender.
    ///
    /// Cancels any timer already running for the pair, so the window measures
    /// the gap between fragments rather than the age of the first one.
    pub async fn reset_text_debounce(&self, chat_id: i64, user_id: i64) -> CancellationToken {
        let token = CancellationToken::new();
        let mut debounce = self.text_debounce.lock().await;
        if let Some(old) = debounce.insert((chat_id, user_id), token.clone()) {
            old.cancel();
        }
        token
    }

    /// Wait for the gap to pass, or until another fragment cancels it.
    /// `true` means the window expired and the message is complete.
    ///
    /// Short on purpose: the client emits the pieces back to back — measured at
    /// 56ms apart — so this only has to outlast network jitter, and it is paid
    /// solely by messages already at the split threshold.
    pub async fn wait_text_debounce(&self, token: CancellationToken) -> bool {
        tokio::select! {
            _ = token.cancelled() => false,
            _ = tokio::time::sleep(std::time::Duration::from_millis(1500)) => true,
        }
    }

    /// Take every buffered fragment for this sender, in arrival order.
    pub async fn drain_text_buffer(&self, chat_id: i64, user_id: i64) -> Vec<String> {
        self.text_buffer
            .lock()
            .await
            .remove(&(chat_id, user_id))
            .unwrap_or_default()
    }

    /// Drop the debounce token once the fragments have been dispatched.
    pub async fn cleanup_text_debounce(&self, chat_id: i64, user_id: i64) {
        self.text_debounce.lock().await.remove(&(chat_id, user_id));
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
