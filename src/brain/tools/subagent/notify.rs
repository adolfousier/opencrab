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

/// Refusal text for a delivery blocked by the channel-ownership gate (fork
/// #17). Pure so tests can pin the remedy wording: the sender must be able to
/// redirect to the occupant without a second discovery round.
fn channel_occupied_message(target: uuid::Uuid, occupant: uuid::Uuid) -> String {
    format!(
        "Refused: session {target} no longer owns its channel — the chat/topic it was bound \
         to is now occupied by session {occupant} (a newer session replaced it, e.g. an \
         idle-timeout reset took over the topic). Waking it would post into {occupant}'s \
         conversation. Notify {occupant} instead if your message belongs to that channel, \
         or pick another target."
    )
}

#[async_trait]
impl Tool for SessionNotifyTool {
    fn name(&self) -> &str {
        "session_notify"
    }

    fn description(&self) -> &str {
        "Push a message to another session's queue in this process. The target \
         drains it at its next tool-loop boundary, or wakes immediately if idle. \
         Refuses while the target is mid-turn unless interrupt=true — do not \
         derail a working session by default. Also refuses when the target no \
         longer owns its channel (a newer session replaced it on its \
         chat/topic) — the refusal names the occupying session, and \
         interrupt does NOT override that gate. Every delivery carries a \
         mechanical header [session-notify from=<sender session id>]; to reply, \
         call session_notify with target_session set to that id. Discover \
         target ids via session_search list/query."
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
                },
                "interrupt": {
                    "type": "boolean",
                    "description": "Deliver even if the target session is mid-turn. Default false: the tool REFUSES while the target is streaming, so a working session is never derailed. Set true only when the target must see this now; the message then queues for its next tool-loop boundary, framed as arrived-during-work."
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
            return Ok(ToolResult::error(
                "Refusing to send an empty message".to_string(),
            ));
        }

        // Mechanical sender signature — from execution context, not model text.
        let from = context.session_id;
        let msg = crate::brain::agent::QueuedUserMessage {
            context_text: format!("[session-notify from={from}]\n\n{message}"),
            display_text: format!("📨 notify from {}:\n{message}", short_id(from)),
            origin: crate::brain::agent::PushOrigin::SessionNotify,
            bg_meta: None,
        };

        // Failsafe default (fork #13): an unset interrupt must not derail a
        // session that is mid-turn. The refusal names the remedy so the
        // calling model learns the knob from the error itself — same pattern
        // as the 429 RetryAfter help text.
        let interrupt = input
            .get("interrupt")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        use crate::brain::agent::service::session_routes::{Delivery, deliver_to_session};

        match deliver_to_session(target, msg, interrupt) {
            Delivery::Delivered => Ok(ToolResult::success(format!(
                "Delivered to session {target}. It will process the message on its next turn."
            ))),
            // Queued, not lost: the target belongs to a channel that has not
            // claimed it since the last restart (#1206). Reporting this as a
            // failure would be the opposite of what happened.
            Delivery::Parked => Ok(ToolResult::success(format!(
                "Queued for session {target}. Its channel has not claimed it since the last \
                 restart, so it will be delivered as soon as that channel next binds the \
                 session."
            ))),
            Delivery::RefusedInFlight => Ok(ToolResult::error(format!(
                "Refused: session {target} is mid-turn (a turn is streaming) and interrupt \
                 was not set — delivering now would derail its current task. Retry when the \
                 session goes idle, or resend with interrupt=true to queue the message for \
                 its in-flight turn's next tool-loop boundary."
            ))),
            Delivery::NoRoute => Ok(ToolResult::error(format!(
                "No live route for session {target} in this process — it has not messaged \
                 since boot, or belongs to another instance/profile. Use a2a_send for \
                 cross-instance targets."
            ))),
            // The target was replaced on its channel (fork #17): name the
            // occupant so the sender can redirect instead of guessing.
            Delivery::RefusedChannelOccupied { occupant } => {
                Ok(ToolResult::error(channel_occupied_message(target, occupant)))
            }
        }
    }
}
