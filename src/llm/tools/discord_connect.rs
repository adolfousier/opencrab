//! Discord Connect Tool
//!
//! Agent-callable tool that connects a Discord bot at runtime.
//! Accepts a bot token, saves it to config/keyring, spawns the bot,
//! and waits for a successful connection.

use super::error::Result;
use super::r#trait::{Tool, ToolCapability, ToolExecutionContext, ToolResult};
use crate::channels::ChannelFactory;
use crate::discord::DiscordState;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;

/// Tool that connects a Discord bot by accepting a bot token.
pub struct DiscordConnectTool {
    channel_factory: Arc<ChannelFactory>,
    discord_state: Arc<DiscordState>,
}

impl DiscordConnectTool {
    pub fn new(
        channel_factory: Arc<ChannelFactory>,
        discord_state: Arc<DiscordState>,
    ) -> Self {
        Self {
            channel_factory,
            discord_state,
        }
    }
}

#[async_trait]
impl Tool for DiscordConnectTool {
    fn name(&self) -> &str {
        "discord_connect"
    }

    fn description(&self) -> &str {
        "Connect a Discord bot to OpenCrabs. Accepts a bot token and starts listening for \
         messages. The user must first create a bot at https://discord.com/developers/applications, \
         enable MESSAGE CONTENT intent, and invite the bot to their server. \
         Call this when the user asks to connect or set up Discord."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "token": {
                    "type": "string",
                    "description": "Discord bot token from the Developer Portal"
                },
                "allowed_users": {
                    "type": "array",
                    "items": { "type": "integer" },
                    "description": "Discord user IDs to allow. If empty, all messages accepted."
                }
            },
            "required": ["token"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::Network, ToolCapability::SystemModification]
    }

    async fn execute(&self, input: Value, _context: &ToolExecutionContext) -> Result<ToolResult> {
        // Check if already connected
        if self.discord_state.is_connected().await {
            return Ok(ToolResult::success(
                "Discord is already connected.".to_string(),
            ));
        }

        let token = match input.get("token").and_then(|v| v.as_str()) {
            Some(t) if !t.is_empty() => t.to_string(),
            _ => {
                return Ok(ToolResult::error(
                    "Missing or empty 'token' parameter. \
                     The user needs to provide their Discord bot token from \
                     https://discord.com/developers/applications"
                        .to_string(),
                ));
            }
        };

        let allowed_users: Vec<i64> = input
            .get("allowed_users")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        // Save token to keyring for persistence
        if let Ok(entry) = keyring::Entry::new("opencrabs", "discord_bot_token") {
            let _ = entry.set_password(&token);
        }

        // Persist enabled state to config
        let _ = crate::config::Config::write_key("channels.discord", "enabled", "true");

        // Create and spawn the Discord agent
        let factory = self.channel_factory.clone();
        let agent = factory.create_agent_service();
        let service_context = factory.service_context();
        let voice_config = factory.voice_config().clone();
        let shared_session = factory.shared_session_id();
        let discord_state = self.discord_state.clone();

        let dc_agent = crate::discord::DiscordAgent::new(
            agent,
            service_context,
            allowed_users,
            voice_config,
            shared_session,
            discord_state.clone(),
        );

        let _handle = dc_agent.start(token);

        // Wait for the bot to connect (ready event sets discord_state)
        let timeout = Duration::from_secs(30);
        let start = std::time::Instant::now();
        loop {
            if discord_state.is_connected().await {
                return Ok(ToolResult::success(
                    "Discord bot connected successfully! Now listening for messages. \
                     Connection persists across restarts."
                        .to_string(),
                ));
            }
            if start.elapsed() > timeout {
                return Ok(ToolResult::error(
                    "Timed out waiting for Discord bot to connect (30s). \
                     Check that the bot token is valid and the bot has the required intents \
                     (MESSAGE CONTENT) enabled in the Developer Portal."
                        .to_string(),
                ));
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
}
