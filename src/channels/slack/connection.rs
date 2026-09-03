//! Connected Socket Mode client, bot token and the owner's last channel.
//!
//! Set when the bot connects and on each owner message; read by the
//! `slack_send` tool for proactive sends and by every API call that needs
//! a token session.

use std::sync::Arc;

use slack_morphism::prelude::SlackHyperClient;

use super::SlackState;

impl SlackState {
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
