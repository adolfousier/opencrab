//! cowork_connect — start a Telegram cowork workspace from anywhere.
//!
//! `/cowork` is a Telegram-group feature, but it can be *launched* from the
//! TUI (or any channel) through this agent tool: it mints a session, registers
//! it in the live `TelegramState`, and returns the `t.me/<bot>?startgroup=…`
//! deep link the user taps to create the group (the bot auto-joins and invites
//! the team) plus a scannable QR. Telegram-gated; mirrors `whatsapp_connect`.

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
        "Start a Telegram cowork workspace (a shared group). Generates a t.me \
         deep link the user taps to create the group — the bot auto-joins and \
         hands out a team invite — plus a scannable QR. Use when the user runs \
         /cowork or asks to create a cowork / team / shared workspace. Works \
         from the TUI and from channels."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "workspace_name": {
                    "type": "string",
                    "description": "Name for the workspace/group (ask the user if not given)."
                }
            },
            "required": ["workspace_name"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::Network]
    }

    fn requires_approval(&self) -> bool {
        false
    }

    async fn execute(&self, input: Value, _context: &ToolExecutionContext) -> Result<ToolResult> {
        let workspace_name = input["workspace_name"].as_str().unwrap_or("").trim();
        if workspace_name.is_empty() {
            return Ok(ToolResult::error(
                "Provide a workspace_name for the cowork group.".into(),
            ));
        }

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
        self.telegram_state
            .set_workspace_name(owner_id, workspace_name)
            .await;

        let deep_link = cowork::build_cowork_deep_link(&bot_username, &session_id);
        // QR for channels (the <<IMG:>> marker is sent as a photo); the TUI
        // shows the clickable link, which terminals render but images they don't.
        let qr_marker = cowork::build_invite_qr(&deep_link)
            .map(|(_, path)| format!("\n\n<<IMG:{}>>", path.display()))
            .unwrap_or_default();

        Ok(ToolResult::success(format!(
            "Cowork workspace '{workspace_name}' is ready. Open this link on your phone \
             (Telegram) to create the group — name it '{workspace_name}' when asked, and I'll \
             auto-join and generate a team invite:\n\n{deep_link}{qr_marker}"
        )))
    }
}
