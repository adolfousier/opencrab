//! Slack Integration
//!
//! Runs a Slack bot via Socket Mode alongside the TUI, forwarding messages from
//! allowlisted users to the AgentService and replying with responses.

mod agent;
pub(crate) mod handler;

pub use agent::SlackAgent;

use slack_morphism::prelude::*;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Shared Slack state for proactive messaging.
///
/// Set when the bot connects via Socket Mode.
/// Read by the `slack_send` tool to send messages on demand.
pub struct SlackState {
    client: Mutex<Option<Arc<SlackHyperClient>>>,
    bot_token: Mutex<Option<String>>,
    /// Channel ID of the owner's last message — used as default for proactive sends
    owner_channel_id: Mutex<Option<String>>,
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
        }
    }

    /// Store the connected client, bot token, and optionally the owner's channel.
    pub async fn set_connected(
        &self,
        client: Arc<SlackHyperClient>,
        bot_token: String,
        channel_id: Option<String>,
    ) {
        *self.client.lock().await = Some(client);
        *self.bot_token.lock().await = Some(bot_token);
        if let Some(id) = channel_id {
            *self.owner_channel_id.lock().await = Some(id);
        }
    }

    /// Update the owner's channel ID (called on each owner message).
    pub async fn set_owner_channel(&self, channel_id: String) {
        *self.owner_channel_id.lock().await = Some(channel_id);
    }

    /// Get a clone of the connected client, if any.
    pub async fn client(&self) -> Option<Arc<SlackHyperClient>> {
        self.client.lock().await.clone()
    }

    /// Get the bot token for opening API sessions.
    pub async fn bot_token(&self) -> Option<String> {
        self.bot_token.lock().await.clone()
    }

    /// Get the owner's last channel ID for proactive messaging.
    pub async fn owner_channel_id(&self) -> Option<String> {
        self.owner_channel_id.lock().await.clone()
    }

    /// Check if Slack is currently connected.
    pub async fn is_connected(&self) -> bool {
        self.client.lock().await.is_some()
    }
}
