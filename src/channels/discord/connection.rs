//! Connected HTTP client, owner channel, bot user id and last-seen guild.
//!
//! Set from the `ready` event and on each owner message; read by the
//! `discord_send` tool for proactive sends and by the handler for
//! @mention detection and guild-scoped actions.

use std::sync::Arc;

use super::DiscordState;

impl DiscordState {
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
}
