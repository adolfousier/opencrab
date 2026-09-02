//! Discord Integration
//!
//! Runs a Discord bot alongside the TUI, forwarding messages from
//! allowlisted users to the AgentService and replying with responses.

mod agent;
mod approval;
mod cancel;
mod connection;
pub(crate) mod handler;
pub(crate) mod interactions;
mod pending_interactions;
pub(crate) mod reactions;
pub(crate) mod resume;
mod sessions;
pub(crate) mod suggest_options;
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
}
