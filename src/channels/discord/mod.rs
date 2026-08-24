//! Discord Integration
//!
//! Runs a Discord bot alongside the TUI, forwarding messages from
//! allowlisted users to the AgentService and replying with responses.

mod agent;
pub(crate) mod handler;
pub(crate) mod interactions;
pub(crate) mod reactions;
pub(crate) mod resume;
pub(crate) mod suggest_followups;
pub(crate) mod tool_group;
pub(crate) mod typing;

pub use agent::DiscordAgent;

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, oneshot};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Shared Discord state for proactive messaging.
///
/// Set when the bot connects via the `ready` event.
/// Read by the `discord_send` tool to send messages on demand.
pub struct DiscordState {
    http: Mutex<Option<Arc<serenity::http::Http>>>,
    /// Channel ID of the owner's last message — used as default for proactive sends
    owner_channel_id: Mutex<Option<u64>>,
    /// Bot's own user ID — set on ready, used for @mention detection
    bot_user_id: Mutex<Option<u64>>,
    /// Guild ID of the last guild message — needed for guild-scoped actions
    guild_id: Mutex<Option<u64>>,
    /// Maps session_id → channel_id for approval routing
    session_channels: Mutex<HashMap<Uuid, u64>>,
    /// Pending approval channels: approval_id → oneshot sender of (approved, always)
    pending_approvals: Mutex<HashMap<String, oneshot::Sender<(bool, bool)>>>,
    /// Per-session cancel tokens for aborting in-flight agent tasks via /stop
    cancel_tokens: Mutex<HashMap<Uuid, CancellationToken>>,
    /// Pending select menus: id -> (created, options) (#382). Lazy TTL:
    /// stale picks answer "expired" (#386).
    pending_selects: Mutex<HashMap<String, (std::time::Instant, Vec<String>)>>,
    /// Pending modal forms: id -> (created, spec) (#383). Same lazy TTL.
    pending_forms: Mutex<HashMap<String, (std::time::Instant, interactions::FormSpec)>>,
    /// Collapsible tool groups keyed by message id, so the Expand/Collapse
    /// interaction can re-render after the turn ended. Insertion-ordered
    /// for pruning; bounded at [`Self::TOOL_GROUP_CAP`].
    tool_groups: Mutex<(Vec<u64>, HashMap<u64, tool_group::GroupState>)>,
}

impl Default for DiscordState {
    fn default() -> Self {
        Self::new()
    }
}

impl DiscordState {
    /// Retained tool groups; older ones stop being toggleable (their last
    /// rendered state stays on screen, like Telegram's frozen blocks).
    const TOOL_GROUP_CAP: usize = 20;

    /// Insert or update a group, PRESERVING the stored expanded/collapsed
    /// choice on updates (a completing tool must not snap an expanded group
    /// shut). Returns the stored state so callers render what is kept.
    pub(crate) async fn upsert_tool_group(
        &self,
        message_id: u64,
        mut group: tool_group::GroupState,
    ) -> tool_group::GroupState {
        let mut guard = self.tool_groups.lock().await;
        let (order, map) = &mut *guard;
        match map.get(&message_id) {
            Some(existing) => group.expanded = existing.expanded,
            None => {
                order.push(message_id);
                while order.len() > Self::TOOL_GROUP_CAP {
                    let oldest = order.remove(0);
                    map.remove(&oldest);
                }
            }
        }
        map.insert(message_id, group.clone());
        group
    }

    /// Register a pending select menu (#382). Bounded by lazy TTL expiry.
    pub(crate) async fn register_select(&self, id: String, options: Vec<String>) {
        self.pending_selects
            .lock()
            .await
            .insert(id, (std::time::Instant::now(), options));
    }

    /// Take a pending select if it is still within `ttl_hours`; expired or
    /// unknown entries return None (and expired ones are dropped).
    pub(crate) async fn take_select(&self, id: &str, ttl_hours: f64) -> Option<Vec<String>> {
        let mut map = self.pending_selects.lock().await;
        let (created, _) = map.get(id)?;
        if created.elapsed().as_secs_f64() > ttl_hours * 3600.0 {
            map.remove(id);
            return None;
        }
        map.remove(id).map(|(_, opts)| opts)
    }

    /// Register a pending modal form spec (#383).
    pub(crate) async fn register_form(&self, id: String, spec: interactions::FormSpec) {
        self.pending_forms
            .lock()
            .await
            .insert(id, (std::time::Instant::now(), spec));
    }

    /// Fetch a pending form spec within TTL (kept until submitted so the
    /// button can be pressed once per open; submission consumes it).
    pub(crate) async fn get_form(
        &self,
        id: &str,
        ttl_hours: f64,
    ) -> Option<interactions::FormSpec> {
        let mut map = self.pending_forms.lock().await;
        let (created, _) = map.get(id)?;
        if created.elapsed().as_secs_f64() > ttl_hours * 3600.0 {
            map.remove(id);
            return None;
        }
        map.get(id).map(|(_, spec)| spec.clone())
    }

    /// Consume a form spec on submission.
    pub(crate) async fn take_form(&self, id: &str) -> Option<interactions::FormSpec> {
        self.pending_forms.lock().await.remove(id).map(|(_, s)| s)
    }

    /// Flip a group's expanded state; None when it aged out of retention.
    pub(crate) async fn toggle_tool_group(
        &self,
        message_id: u64,
    ) -> Option<tool_group::GroupState> {
        let mut guard = self.tool_groups.lock().await;
        let (_, map) = &mut *guard;
        let group = map.get_mut(&message_id)?;
        group.expanded = !group.expanded;
        Some(group.clone())
    }

    pub fn new() -> Self {
        Self {
            http: Mutex::new(None),
            owner_channel_id: Mutex::new(None),
            bot_user_id: Mutex::new(None),
            guild_id: Mutex::new(None),
            session_channels: Mutex::new(HashMap::new()),
            pending_approvals: Mutex::new(HashMap::new()),
            cancel_tokens: Mutex::new(HashMap::new()),
            pending_selects: Mutex::new(HashMap::new()),
            pending_forms: Mutex::new(HashMap::new()),
            tool_groups: Mutex::new((Vec::new(), HashMap::new())),
        }
    }

    /// Store the connected HTTP client and optionally set the owner channel.
    pub async fn set_connected(&self, http: Arc<serenity::http::Http>, channel_id: Option<u64>) {
        *self.http.lock().await = Some(http);
        if let Some(id) = channel_id {
            *self.owner_channel_id.lock().await = Some(id);
        }
    }

    /// Update the owner's channel ID (called on each owner message).
    pub async fn set_owner_channel(&self, channel_id: u64) {
        *self.owner_channel_id.lock().await = Some(channel_id);
    }

    /// Get a clone of the HTTP client, if connected.
    pub async fn http(&self) -> Option<Arc<serenity::http::Http>> {
        self.http.lock().await.clone()
    }

    /// Get the owner's last channel ID for proactive messaging.
    pub async fn owner_channel_id(&self) -> Option<u64> {
        *self.owner_channel_id.lock().await
    }

    /// Store the bot's own user ID (set from ready event).
    pub async fn set_bot_user_id(&self, id: u64) {
        *self.bot_user_id.lock().await = Some(id);
    }

    /// Get the bot's user ID for @mention detection.
    pub async fn bot_user_id(&self) -> Option<u64> {
        *self.bot_user_id.lock().await
    }

    /// Store the guild ID from an incoming guild message.
    pub async fn set_guild_id(&self, id: u64) {
        *self.guild_id.lock().await = Some(id);
    }

    /// Get the last-seen guild ID for guild-scoped actions.
    pub async fn guild_id(&self) -> Option<u64> {
        *self.guild_id.lock().await
    }

    /// Check if Discord is currently connected.
    pub async fn is_connected(&self) -> bool {
        self.http.lock().await.is_some()
    }

    /// Record which channel_id corresponds to a given session.
    pub async fn register_session_channel(&self, session_id: Uuid, channel_id: u64) {
        self.session_channels
            .lock()
            .await
            .insert(session_id, channel_id);
    }

    /// Look up the channel_id for a session.
    pub async fn session_channel(&self, session_id: Uuid) -> Option<u64> {
        self.session_channels.lock().await.get(&session_id).copied()
    }

    /// Register a pending approval oneshot channel.
    pub async fn register_pending_approval(&self, id: String, tx: oneshot::Sender<(bool, bool)>) {
        self.pending_approvals.lock().await.insert(id, tx);
    }

    /// Resolve a pending approval. Returns true if one existed.
    pub async fn resolve_pending_approval(&self, id: &str, approved: bool, always: bool) -> bool {
        if let Some(tx) = self.pending_approvals.lock().await.remove(id) {
            let _ = tx.send((approved, always));
            true
        } else {
            false
        }
    }

    /// Store a cancel token for a session (before starting agent call).
    /// If a token already exists for this session, cancel it first to abort the
    /// previous in-flight agent call — prevents concurrent uncancellable agents.
    pub async fn store_cancel_token(&self, session_id: Uuid, token: CancellationToken) {
        let mut tokens = self.cancel_tokens.lock().await;
        if let Some(old) = tokens.remove(&session_id) {
            tracing::warn!(
                "Discord: cancelling previous in-flight agent call for session {}",
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
}
