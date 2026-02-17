//! Telegram Bot Integration
//!
//! Runs a Telegram bot alongside the TUI, forwarding messages from
//! allowlisted users to the AgentService and replying with responses.

mod agent;
pub(crate) mod handler;

pub use agent::TelegramAgent;

use teloxide::prelude::Bot;
use tokio::sync::Mutex;

/// Shared Telegram state for proactive messaging.
///
/// Set when the bot connects (agent stores Bot) and when the owner
/// sends their first message (handler stores chat_id).
/// Read by the `telegram_send` tool to send messages on demand.
pub struct TelegramState {
    bot: Mutex<Option<Bot>>,
    /// Chat ID of the owner's conversation — used as default for proactive sends
    owner_chat_id: Mutex<Option<i64>>,
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

    /// Check if Telegram is currently connected.
    pub async fn is_connected(&self) -> bool {
        self.bot.lock().await.is_some()
    }
}
