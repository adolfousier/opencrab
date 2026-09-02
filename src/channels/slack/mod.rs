//! Slack Integration
//!
//! Runs a Slack bot via Socket Mode alongside the TUI, forwarding messages from
//! allowlisted users to the AgentService and replying with responses.

mod agent;
mod approval;
pub(crate) mod blocks;
mod cancel;
mod connection;
pub(crate) mod final_body;
mod followups;
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
    /// Insertion-ordered for pruning; bounded at [`Self::TOOL_GROUP_CAP`] (see `tool_group`).
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
}
