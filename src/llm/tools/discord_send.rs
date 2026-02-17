//! Discord Send Tool
//!
//! Agent-callable tool for proactively sending Discord messages.
//! Uses the shared `DiscordState` to access the connected HTTP client.

use super::error::Result;
use super::r#trait::{Tool, ToolCapability, ToolExecutionContext, ToolResult};
use crate::discord::DiscordState;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

/// Tool that sends a Discord message to the owner's channel or a specific channel.
pub struct DiscordSendTool {
    discord_state: Arc<DiscordState>,
}

impl DiscordSendTool {
    pub fn new(discord_state: Arc<DiscordState>) -> Self {
        Self { discord_state }
    }
}

#[async_trait]
impl Tool for DiscordSendTool {
    fn name(&self) -> &str {
        "discord_send"
    }

    fn description(&self) -> &str {
        "Send a Discord message to the user. Use this to proactively reach out, share updates, \
         or notify the user about completed tasks. If no channel_id is specified, the message \
         is sent to the owner's last active channel. Requires Discord bot to be connected first."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "message": {
                    "type": "string",
                    "description": "The message text to send"
                },
                "channel_id": {
                    "type": "string",
                    "description": "Discord channel ID to send to (numeric string). Omit to message the owner's last channel."
                }
            },
            "required": ["message"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::Network]
    }

    async fn execute(&self, input: Value, _context: &ToolExecutionContext) -> Result<ToolResult> {
        let message = match input.get("message").and_then(|v| v.as_str()) {
            Some(m) if !m.is_empty() => m.to_string(),
            _ => {
                return Ok(ToolResult::error(
                    "Missing or empty 'message' parameter.".to_string(),
                ));
            }
        };

        let http = match self.discord_state.http().await {
            Some(h) => h,
            None => {
                return Ok(ToolResult::error(
                    "Discord is not connected. The bot needs to be running with a valid token."
                        .to_string(),
                ));
            }
        };

        // Resolve target channel ID
        let channel_id = if let Some(id_str) = input.get("channel_id").and_then(|v| v.as_str()) {
            match id_str.parse::<u64>() {
                Ok(id) => id,
                Err(_) => {
                    return Ok(ToolResult::error(format!(
                        "Invalid channel_id '{}': must be a numeric string",
                        id_str
                    )));
                }
            }
        } else {
            match self.discord_state.owner_channel_id().await {
                Some(id) => id,
                None => {
                    return Ok(ToolResult::error(
                        "No owner channel ID available and no 'channel_id' parameter provided. \
                         The owner needs to send a message first to establish a channel."
                            .to_string(),
                    ));
                }
            }
        };

        // Prepend agent header and split long messages
        let tagged = format!(
            "{}\n\n{}",
            crate::discord::handler::MSG_HEADER,
            message
        );
        let chunks = crate::discord::handler::split_message(&tagged, 2000);

        let channel = serenity::model::id::ChannelId::new(channel_id);
        for chunk in chunks {
            if let Err(e) = channel.say(&http, chunk).await {
                return Ok(ToolResult::error(format!(
                    "Failed to send Discord message: {}",
                    e
                )));
            }
        }

        Ok(ToolResult::success(format!(
            "Message sent to Discord channel {}.",
            channel_id
        )))
    }
}
