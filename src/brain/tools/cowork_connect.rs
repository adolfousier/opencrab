//! cowork_connect — add OpenCrabs to a Telegram group from any channel.
//!
//! `/cowork` is a Telegram-group feature, but it can be *launched* from the
//! TUI (or any channel) through this agent tool: it mints a session, registers
//! it in the live `TelegramState`, and returns the `t.me/<bot>?startgroup=…`
//! deep link the user taps to add the bot to a group. All group members
//! auto-register in allowed_users. Telegram-gated; mirrors `whatsapp_connect`.

use super::error::Result;
use super::r#trait::{Tool, ToolCapability, ToolExecutionContext, ToolResult};
use crate::channels::telegram::{TelegramState, cowork};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

pub struct CoworkConnectTool {
    telegram_state: Arc<TelegramState>,
}

impl CoworkConnectTool {
    pub fn new(telegram_state: Arc<TelegramState>) -> Self {
        Self { telegram_state }
    }
}

#[async_trait]
impl Tool for CoworkConnectTool {
    fn name(&self) -> &str {
        "cowork_connect"
    }

    fn description(&self) -> &str {
        "Add OpenCrabs to a Telegram group. Generates a t.me deep link the user \
         taps to pick or create a group — the bot auto-joins and all members \
         auto-register. Use when the user runs /cowork or asks to add the bot \
         to a Telegram group. Works from the TUI and from channels."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::Network]
    }

    fn requires_approval(&self) -> bool {
        false
    }

    async fn execute(&self, _input: Value, _context: &ToolExecutionContext) -> Result<ToolResult> {
        let Some(bot_username) = self.telegram_state.bot_username().await else {
            return Ok(ToolResult::error(
                "Telegram isn't connected yet — set it up first (/onboard:channels telegram) \
                 so I can build the cowork link."
                    .into(),
            ));
        };

        // Best-effort owner id for the cowork state. The group-join path keys
        // off the session_id (take_cowork_by_session), so a missing owner id is
        // non-fatal — it only labels who started the flow.
        let owner_id = crate::config::Config::load()
            .ok()
            .and_then(|c| {
                c.channels
                    .telegram
                    .allowed_users
                    .first()
                    .and_then(|s| s.parse::<i64>().ok())
            })
            .unwrap_or(0);

        let session_id = uuid::Uuid::new_v4().simple().to_string();
        // Register the session so the bot recognizes the group when the deep
        // link creates it (start_cowork inserts into cowork_sessions).
        self.telegram_state
            .start_cowork(owner_id, owner_id, session_id.clone())
            .await;

        let deep_link = cowork::build_cowork_deep_link(&bot_username, &session_id);
        // QR for channels (the <<IMG:>> marker is sent as a photo); the TUI
        // shows the clickable link, which terminals render but images they don't.
        let qr_marker = cowork::build_invite_qr(&deep_link)
            .map(|(_, path)| format!("\n\n<<IMG:{}>>", path.display()))
            .unwrap_or_default();

        Ok(ToolResult::success(format!(
            "Tap this link on your phone (Telegram) to add me to a group:\n\n\
             {deep_link}\n\n\
             Every member auto-registers when they send a message.{qr_marker}"
        )))
    }
}
