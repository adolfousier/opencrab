//! Telegram Send Tool
//!
//! Agent-callable tool for proactively sending Telegram messages.
//! Uses the shared `TelegramState` to access the connected bot.

use super::error::Result;
use super::r#trait::{Tool, ToolCapability, ToolExecutionContext, ToolResult};
use crate::telegram::TelegramState;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::ChatId;

/// Tool that sends a Telegram message to the owner or a specific chat.
pub struct TelegramSendTool {
    telegram_state: Arc<TelegramState>,
}

impl TelegramSendTool {
    pub fn new(telegram_state: Arc<TelegramState>) -> Self {
        Self { telegram_state }
    }
}

#[async_trait]
impl Tool for TelegramSendTool {
    fn name(&self) -> &str {
        "telegram_send"
    }

    fn description(&self) -> &str {
        "Send a Telegram message to the user. Use this to proactively reach out, share updates, \
         or notify the user about completed tasks. If no chat_id is specified, the message \
         is sent to the owner (primary user). Requires Telegram to be connected first."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "message": {
                    "type": "string",
                    "description": "The message text to send"
                },
                "chat_id": {
                    "type": "integer",
                    "description": "Telegram chat ID to send to. Omit to message the owner."
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

        let bot = match self.telegram_state.bot().await {
            Some(b) => b,
            None => {
                return Ok(ToolResult::error(
                    "Telegram is not connected. Ask the user to connect Telegram first \
                     (use the telegram_connect tool)."
                        .to_string(),
                ));
            }
        };

        // Resolve target chat: explicit chat_id or owner
        let chat_id = if let Some(id) = input.get("chat_id").and_then(|v| v.as_i64()) {
            id
        } else {
            match self.telegram_state.owner_chat_id().await {
                Some(id) => id,
                None => {
                    return Ok(ToolResult::error(
                        "No owner chat ID known yet and no 'chat_id' parameter provided. \
                         The owner needs to send at least one message to the bot first, \
                         or specify a chat_id."
                            .to_string(),
                    ));
                }
            }
        };

        // Prepend agent header and split long messages
        let tagged = format!(
            "{}\n\n{}",
            crate::telegram::handler::MSG_HEADER,
            message
        );
        let chunks = crate::telegram::handler::split_message(&tagged, 4096);
        for chunk in chunks {
            if let Err(e) = bot.send_message(ChatId(chat_id), chunk).await {
                return Ok(ToolResult::error(format!(
                    "Failed to send Telegram message: {}",
                    e
                )));
            }
        }

        Ok(ToolResult::success(format!(
            "Message sent to chat {} via Telegram.",
            chat_id
        )))
    }
}
