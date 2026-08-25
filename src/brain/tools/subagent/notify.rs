//! session_notify tool — push a message into another live session's queue.
//!
//! Thin agent-facing wrapper over `session_routes::deliver_to_session`, so an
//! orchestrator (e.g. a compiling agent) can hand work back to the sessions
//! whose commits broke the build (issue adolfousier/opencrabs#1203).
//!
//! Sender identity is injected MECHANICALLY from `ToolExecutionContext`
//! — the calling model can neither forge nor omit the `[session-notify
//! from=<uuid>]` header prepended to every delivery.

use crate::brain::tools::error::{Result, ToolError};
use crate::brain::tools::r#trait::{Tool, ToolCapability, ToolExecutionContext, ToolResult};
use async_trait::async_trait;
use serde_json::Value;

/// Tool that pushes a queued user message into another session.
pub struct SessionNotifyTool;

/// Short display form of a session UUID (first 8 hex chars).
fn short_id(id: uuid::Uuid) -> String {
    id.simple().to_string()[..8].to_string()
}

#[async_trait]
impl Tool for SessionNotifyTool {
    fn name(&self) -> &str {
        "session_notify"
    }

    fn description(&self) -> &str {
        "Push a message to another session's queue in this process. The target \
         drains it at its next tool-loop boundary, or wakes immediately if idle. \
         Every delivery carries a mechanical header [session-notify from=<sender \
         session id>]; to reply, call session_notify with target_session set to \
         that id. Discover target ids via session_search list/query."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "target_session": {
                    "type": "string",
                    "description": "UUID of the target session (from session_search list/query, or the from=<id> header of a session_notify you received)"
                },
                "message": {
                    "type": "string",
                    "description": "Text to deliver to the target session"
                }
            },
            "required": ["target_session", "message"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![]
    }

    fn requires_approval(&self) -> bool {
        false
    }

    async fn execute(&self, input: Value, context: &ToolExecutionContext) -> Result<ToolResult> {
        let target = input
            .get("target_session")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("'target_session' is required".into()))?;
        let target: uuid::Uuid = target.parse().map_err(|_| {
            ToolError::InvalidInput(format!("'target_session' is not a valid UUID: {target}"))
        })?;

        let message = input
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("'message' is required".into()))?;
        if message.trim().is_empty() {
            return Ok(ToolResult::error("Refusing to send an empty message".to_string()));
        }

        // Mechanical sender signature — from execution context, not model text.
        let from = context.session_id;
        let msg = crate::brain::agent::QueuedUserMessage {
            context_text: format!("[session-notify from={from}]\n\n{message}"),
            display_text: format!("📨 notify from {}:\n{message}", short_id(from)),
        };

        if crate::brain::agent::service::session_routes::deliver_to_session(target, msg) {
            Ok(ToolResult::success(format!(
                "Delivered to session {target}. It will process the message on its next turn."
            )))
        } else {
            Ok(ToolResult::error(format!(
                "No live route for session {target} in this process — it has not messaged \
                 since boot, or belongs to another instance/profile. Use a2a_send for \
                 cross-instance targets."
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_id_truncates_to_8_chars() {
        let id = uuid::Uuid::parse_str("3f2a1b4c5d6e7f8090a1b2c3d4e5f607").unwrap();
        assert_eq!(short_id(id), "3f2a1b4c");
    }

    #[test]
    fn name_and_schema_are_consistent() {
        let tool = SessionNotifyTool;
        assert_eq!(tool.name(), "session_notify");
        assert!(!tool.requires_approval());
        let schema = tool.input_schema();
        let required = schema["required"].as_array().unwrap();
        assert_eq!(required.len(), 2);
        assert!(!tool.description().is_empty());
    }
}
