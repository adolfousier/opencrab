//! Slack Integration
//!
//! Runs a Slack bot via Socket Mode alongside the TUI, forwarding messages from
//! allowlisted users to the AgentService and replying with responses.

mod agent;
pub(crate) mod blocks;
mod connection;
pub(crate) mod final_body;
pub(crate) mod formatting_prompt;
pub(crate) mod handler;
pub(crate) mod reactions;
pub(crate) mod resume;
mod sessions;
pub(crate) mod suggest_options;
pub(crate) mod table_convert;
pub(crate) mod tool_group;
pub(crate) mod upload;

pub use agent::SlackAgent;

use slack_morphism::prelude::SlackHyperClient;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, oneshot};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Shared Slack state for proactive messaging.
///
/// Set when the bot connects via Socket Mode.
/// Read by the `slack_send` tool to send messages on demand.
pub struct SlackState {
    client: Mutex<Option<Arc<SlackHyperClient>>>,
    bot_token: Mutex<Option<String>>,
    /// Channel ID of the owner's last message — used as default for proactive sends
    owner_channel_id: Mutex<Option<String>>,
    /// Maps session_id → channel_id for approval routing
    session_channels: Mutex<HashMap<Uuid, String>>,
    /// Pending approval channels: approval_id → oneshot sender of (approved, always)
    pending_approvals: Mutex<HashMap<String, oneshot::Sender<(bool, bool)>>>,
    /// Pending follow-up questions: question_id → (oneshot sender,
    /// options). Same shape as the other channels — action_id only
    /// carries the option index, the click handler maps it back via
    /// the stored options list.
    /// Per-session OPTIONAL follow-up suggestions from `suggest_options`
    /// (#599). Non-blocking: buttons ride under the response and a tap injects
    /// the chosen suggestion as a new turn. Keyed by session; the tap handler
    /// resolves `idx -> text`. Cleared on tap or when the user sends anything.
    pending_followups: Mutex<HashMap<Uuid, Vec<String>>>,
    /// Per-session cancel tokens for aborting in-flight agent tasks via /stop
    cancel_tokens: Mutex<HashMap<Uuid, CancellationToken>>,
    /// Collapsible tool groups keyed by their message ts, so the Expand /
    /// Collapse interaction can re-render long after the turn ended.
    /// Insertion-ordered for pruning; bounded at [`Self::TOOL_GROUP_CAP`].
    tool_groups: Mutex<(Vec<String>, HashMap<String, tool_group::GroupState>)>,
}

impl Default for SlackState {
    fn default() -> Self {
        Self::new()
    }
}

impl SlackState {
    pub fn new() -> Self {
        Self {
            client: Mutex::new(None),
            bot_token: Mutex::new(None),
            owner_channel_id: Mutex::new(None),
            session_channels: Mutex::new(HashMap::new()),
            pending_approvals: Mutex::new(HashMap::new()),
            pending_followups: Mutex::new(HashMap::new()),
            cancel_tokens: Mutex::new(HashMap::new()),
            tool_groups: Mutex::new((Vec::new(), HashMap::new())),
        }
    }

    /// Retained tool groups; older ones stop being toggleable (their last
    /// rendered state stays on screen, like Telegram's frozen blocks).
    const TOOL_GROUP_CAP: usize = 20;

    /// Insert or update the group for a message ts, PRESERVING the user's
    /// expanded/collapsed choice on updates (a completing tool must not
    /// snap an expanded group shut). Prunes the oldest beyond the cap.
    /// Returns the stored state so callers render exactly what is kept.
    pub(crate) async fn upsert_tool_group(
        &self,
        ts: String,
        mut group: tool_group::GroupState,
    ) -> tool_group::GroupState {
        let mut guard = self.tool_groups.lock().await;
        let (order, map) = &mut *guard;
        match map.get(&ts) {
            Some(existing) => group.expanded = existing.expanded,
            None => {
                order.push(ts.clone());
                while order.len() > Self::TOOL_GROUP_CAP {
                    let oldest = order.remove(0);
                    map.remove(&oldest);
                }
            }
        }
        map.insert(ts, group.clone());
        group
    }

    /// Flip a group's expanded state; returns the new state for re-render,
    /// or None when the group aged out of retention.
    pub(crate) async fn toggle_tool_group(&self, ts: &str) -> Option<tool_group::GroupState> {
        let mut guard = self.tool_groups.lock().await;
        let (_, map) = &mut *guard;
        let group = map.get_mut(ts)?;
        group.expanded = !group.expanded;
        Some(group.clone())
    }

    /// Stash this session's optional follow-up suggestions (#599).
    pub async fn set_pending_followups(&self, session_id: Uuid, options: Vec<String>) {
        self.pending_followups
            .lock()
            .await
            .insert(session_id, options);
    }

    /// Take a tapped follow-up suggestion by index, consuming the whole set.
    pub async fn take_pending_followup(&self, session_id: Uuid, idx: usize) -> Option<String> {
        let options = self.pending_followups.lock().await.remove(&session_id)?;
        options.get(idx).cloned()
    }

    /// Drop this session's pending follow-up suggestions (user sent their own).
    pub async fn clear_pending_followups(&self, session_id: Uuid) {
        self.pending_followups.lock().await.remove(&session_id);
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
                "Slack: cancelling previous in-flight agent call for session {}",
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
