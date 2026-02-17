//! Slack Send Tool
//!
//! Agent-callable tool for proactively sending Slack messages.
//! Uses the shared `SlackState` to access the connected client.

use super::error::Result;
use super::r#trait::{Tool, ToolCapability, ToolExecutionContext, ToolResult};
use crate::slack::SlackState;
use async_trait::async_trait;
use serde_json::Value;
use slack_morphism::prelude::*;
use std::sync::Arc;

/// Tool that sends a Slack message to the owner's channel or a specific channel.
pub struct SlackSendTool {
    slack_state: Arc<SlackState>,
}

impl SlackSendTool {
    pub fn new(slack_state: Arc<SlackState>) -> Self {
        Self { slack_state }
    }
}

#[async_trait]
impl Tool for SlackSendTool {
    fn name(&self) -> &str {
        "slack_send"
    }

    fn description(&self) -> &str {
        "Send a Slack message to the user. Use this to proactively reach out, share updates, \
         or notify the user about completed tasks. If no channel is specified, the message \
         is sent to the owner's last active channel. Requires Slack bot to be connected first."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "message": {
                    "type": "string",
                    "description": "The message text to send"
                },
                "channel": {
                    "type": "string",
                    "description": "Slack channel ID (e.g. 'C12345678'). Omit to message the owner's last channel."
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

        let client = match self.slack_state.client().await {
            Some(c) => c,
            None => {
                return Ok(ToolResult::error(
                    "Slack is not connected. The bot needs to be running with valid tokens."
                        .to_string(),
                ));
            }
        };

        let bot_token = match self.slack_state.bot_token().await {
            Some(t) => t,
            None => {
                return Ok(ToolResult::error(
                    "Slack bot token not available.".to_string(),
                ));
            }
        };

        // Resolve target channel
        let channel_id = if let Some(ch) = input.get("channel").and_then(|v| v.as_str()) {
            ch.to_string()
        } else {
            match self.slack_state.owner_channel_id().await {
                Some(id) => id,
                None => {
                    return Ok(ToolResult::error(
                        "No owner channel ID available and no 'channel' parameter provided. \
                         The owner needs to send a message first to establish a channel."
                            .to_string(),
                    ));
                }
            }
        };

        // Prepend agent header and split long messages
        let tagged = format!(
            "{}\n\n{}",
            crate::slack::handler::MSG_HEADER,
            message
        );
        let chunks = crate::slack::handler::split_message(&tagged, 3000);

        let token = SlackApiToken::new(SlackApiTokenValue::from(bot_token));
        let session = client.open_session(&token);

        for chunk in chunks {
            let request = SlackApiChatPostMessageRequest::new(
                SlackChannelId::new(channel_id.clone()),
                SlackMessageContent::new().with_text(chunk.to_string()),
            );
            if let Err(e) = session.chat_post_message(&request).await {
                return Ok(ToolResult::error(format!(
                    "Failed to send Slack message: {}",
                    e
                )));
            }
        }

        Ok(ToolResult::success(format!(
            "Message sent to Slack channel {}.",
            channel_id
        )))
    }
}
