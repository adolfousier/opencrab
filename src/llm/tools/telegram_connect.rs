//! Telegram Connect Tool
//!
//! Agent-callable tool that connects a Telegram bot at runtime.
//! Accepts a bot token, saves it to config/keyring, and spawns the bot.

use super::error::Result;
use super::r#trait::{Tool, ToolCapability, ToolExecutionContext, ToolResult};
use crate::channels::ChannelFactory;
use crate::telegram::TelegramState;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;

/// Tool that connects a Telegram bot by accepting a bot token from BotFather.
pub struct TelegramConnectTool {
    channel_factory: Arc<ChannelFactory>,
    telegram_state: Arc<TelegramState>,
}

impl TelegramConnectTool {
    pub fn new(
        channel_factory: Arc<ChannelFactory>,
        telegram_state: Arc<TelegramState>,
    ) -> Self {
        Self {
            channel_factory,
            telegram_state,
        }
    }
}

#[async_trait]
impl Tool for TelegramConnectTool {
    fn name(&self) -> &str {
        "telegram_connect"
    }

    fn description(&self) -> &str {
        "Connect a Telegram bot to OpenCrabs. Accepts a bot token from @BotFather and starts \
         listening for messages. The user must first create a bot via @BotFather on Telegram. \
         Call this when the user asks to connect or set up Telegram."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "token": {
                    "type": "string",
                    "description": "Telegram bot token from @BotFather (format: 123456:ABC-DEF...)"
                },
                "allowed_users": {
                    "type": "array",
                    "items": { "type": "integer" },
                    "description": "Telegram user IDs to allow. If empty, all messages accepted. Users can send /start to the bot to get their ID."
                }
            },
            "required": ["token"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::Network, ToolCapability::SystemModification]
    }

    async fn execute(&self, input: Value, _context: &ToolExecutionContext) -> Result<ToolResult> {
        if self.telegram_state.is_connected().await {
            return Ok(ToolResult::success(
                "Telegram is already connected.".to_string(),
            ));
        }

        let token = match input.get("token").and_then(|v| v.as_str()) {
            Some(t) if !t.is_empty() => t.to_string(),
            _ => {
                return Ok(ToolResult::error(
                    "Missing or empty 'token' parameter. \
                     The user needs to create a bot via @BotFather on Telegram and provide the token."
                        .to_string(),
                ));
            }
        };

        let allowed_users: Vec<i64> = input
            .get("allowed_users")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        // Save token to keyring for persistence
        if let Ok(entry) = keyring::Entry::new("opencrabs", "telegram_bot_token") {
            let _ = entry.set_password(&token);
        }

        // Persist enabled state to config
        let _ = crate::config::Config::write_key("channels.telegram", "enabled", "true");

        // Create and spawn the Telegram agent
        let factory = self.channel_factory.clone();
        let agent = factory.create_agent_service();
        let service_context = factory.service_context();
        let voice_config = factory.voice_config().clone();
        let shared_session = factory.shared_session_id();
        let telegram_state = self.telegram_state.clone();

        let openai_key = std::env::var("OPENAI_API_KEY").ok();

        let tg_agent = crate::telegram::TelegramAgent::new(
            agent,
            service_context,
            allowed_users,
            voice_config,
            openai_key,
            shared_session,
            telegram_state.clone(),
        );

        let _handle = tg_agent.start(token);

        // Wait for the bot to connect (agent stores Bot in state)
        let timeout = Duration::from_secs(15);
        let start = std::time::Instant::now();
        loop {
            if telegram_state.is_connected().await {
                return Ok(ToolResult::success(
                    "Telegram bot connected successfully! Now listening for messages. \
                     Users can send /start to the bot to get their user ID for the allowlist. \
                     Connection persists across restarts."
                        .to_string(),
                ));
            }
            if start.elapsed() > timeout {
                return Ok(ToolResult::error(
                    "Timed out waiting for Telegram bot to connect (15s). \
                     Check that the bot token from @BotFather is valid."
                        .to_string(),
                ));
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
}
