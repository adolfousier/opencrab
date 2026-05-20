//! Rename-session tool.
//!
//! Lets the agent rename the current session so the title in `/sessions`
//! reflects what the conversation is actually about. Channels and the
//! TUI stamp a static label at session creation (e.g. "A2A: <first 60
//! chars>", "Telegram: <chat>"); this tool is the agent-callable path
//! to update that label once the conversation has enough context.

use super::error::Result;
use super::r#trait::{Tool, ToolCapability, ToolExecutionContext, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

pub struct RenameSessionTool;

#[derive(Debug, Deserialize)]
struct RenameInput {
    /// New title for the session. Pass an empty string to clear the
    /// title and let downstream lookups fall back to the channel
    /// default.
    title: String,
}

#[async_trait]
impl Tool for RenameSessionTool {
    fn name(&self) -> &str {
        "rename_session"
    }

    fn description(&self) -> &str {
        "Rename the current session. Use this once a session has enough context that the \
         channel-default title (e.g. 'A2A: <first 60 chars>', 'Telegram: <chat>') is no \
         longer descriptive. Provide a short, specific title (3-8 words) that reflects \
         the actual work being done. Pass an empty string to clear the title."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "title": {
                    "type": "string",
                    "description": "New session title. Empty string clears the title back to no-title.",
                    "maxLength": 200
                }
            },
            "required": ["title"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        // Metadata-only update — no filesystem, no shell, no network.
        vec![]
    }

    fn requires_approval(&self) -> bool {
        // Low blast radius: title is a display string, easily reverted
        // via the same tool or via the TUI rename UI. Don't gate it
        // behind approval — the agent should use it proactively.
        false
    }

    async fn execute(&self, input: Value, context: &ToolExecutionContext) -> Result<ToolResult> {
        let parsed: RenameInput = serde_json::from_value(input)?;
        let trimmed = parsed.title.trim();
        if trimmed.len() > 200 {
            return Ok(ToolResult::error(
                "Title too long (max 200 chars). Pick something shorter and more specific.".into(),
            ));
        }

        let svc_ctx = match context.service_context.as_ref() {
            Some(c) => c,
            None => {
                return Ok(ToolResult::error(
                    "No service context available — rename_session can only run inside an \
                     active session loop."
                        .into(),
                ));
            }
        };

        let session_svc = crate::services::SessionService::new(svc_ctx.clone());
        let session_id = context.session_id;
        let new_title: Option<String> = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        };

        match session_svc
            .update_session_title(session_id, new_title.clone())
            .await
        {
            Ok(()) => {
                let summary = match new_title {
                    Some(t) => format!("Session renamed to '{}'.", t),
                    None => "Session title cleared.".to_string(),
                };
                Ok(ToolResult::success(summary))
            }
            Err(e) => Ok(ToolResult::error(format!(
                "Failed to rename session {}: {}",
                session_id, e
            ))),
        }
    }
}
