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

/// Redirect-acknowledgement text for a delivery steered to the channel's
/// current owner (fork #19). Pure so tests can pin the wording: the sender
/// must hear WHERE the message went without a second discovery round.
fn redirect_message(target: uuid::Uuid, occupant: uuid::Uuid) -> String {
    format!(
        "Redirected: session {target} no longer owns its channel — the chat/topic it was \
         bound to is now occupied by session {occupant} (a newer session replaced it, e.g. \
         an idle-timeout reset took over the topic). The message was delivered to {occupant} \
         instead, with provenance framing so the new owner can tell it apart from its own \
         work."
    )
}

/// Machine-readable send verdict (Notifications v2, fork #50): the external
/// `notify_state` mirrors the internal `Delivery` enum 1:1 — delivered /
/// queued / redirected / refused (+ `notify_reason`, `notify_occupant`, …) —
/// so callers never parse prose. The human text stays in `output` for the
/// calling model; the structured fields ride `metadata`.
fn verdict(success: bool, state: &str, detail: String, extra: &[(&str, String)]) -> ToolResult {
    let mut result = if success {
        ToolResult::success(detail)
    } else {
        ToolResult::error(detail)
    };
    result = result.with_metadata("notify_state".into(), state.into());
    for (key, value) in extra {
        result = result.with_metadata((*key).to_string(), value.clone());
    }
    result
}

/// Resolve the v2 delivery policy against the deprecated `interrupt` alias
/// (fork #50). `interrupt=true` was always "queue for the in-flight turn's
/// next tool-loop boundary" — that is mode `turn-end`; unset/false was
/// "refuse while streaming" — mode `now`. Both may be passed only when they
/// agree; a disagreement is an error, never a silent precedence. Unknown
/// modes (e.g. `quiet`, landing with the quiet engine) are rejected here so
/// the schema can teach them before they exist.
fn resolve_mode(mode: Option<&str>, interrupt: Option<bool>) -> Result<bool> {
    let mode = match mode {
        None => None,
        Some(known @ ("now" | "turn-end")) => Some(known),
        Some(other) => {
            return Err(ToolError::InvalidInput(format!(
                "delivery.mode '{other}' is not available yet — use 'now' or 'turn-end'"
            )));
        }
    };
    match (mode, interrupt) {
        (Some("turn-end"), None | Some(true)) | (None, Some(true)) => Ok(true),
        (Some("now"), None | Some(false)) | (None, None | Some(false)) => Ok(false),
        _ => Err(ToolError::InvalidInput(
            "delivery.mode and interrupt disagree — pass one, not both".into(),
        )),
    }
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
         derail a working session by default. When the target no longer \
         owns its channel (a newer session replaced it on its \
         chat/topic), the message is REDIRECTED to the occupying session \
         with provenance framing, and delivery reports the redirect; \
         interrupt does NOT override that gate. Every delivery carries a \
         mechanical header [session-notify from=<sender session id>]; to reply, \
         call session_notify with target_session set to that id. Discover \
         target ids via session_search list/query."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["send"],
                    "description": "Operation on the notification family. v1 ships 'send' only (the default when omitted); 'cancel' and 'list' join when notification ids gain consumers."
                },
                "target_session": {
                    "type": "string",
                    "description": "UUID of the target session (from session_search list/query, or the from=<id> header of a session_notify you received)"
                },
                "message": {
                    "type": "string",
                    "description": "Text to deliver to the target session"
                },
                "delivery": {
                    "type": "object",
                    "description": "Delivery policy. Omit for the default (mode 'now').",
                    "properties": {
                        "mode": {
                            "type": "string",
                            "enum": ["now", "turn-end"],
                            "description": "'now' (default): deliver immediately; REFUSES while the target is mid-turn. 'turn-end': queue the message for the target's next tool-loop boundary even while it streams. More modes (quiet: defer until the target has been idle for a window) arrive with the quiet engine."
                        }
                    }
                },
                "interrupt": {
                    "type": "boolean",
                    "description": "Deprecated alias for delivery.mode: true = 'turn-end', false/unset = 'now'. Prefer delivery.mode; passing both is allowed only when they agree."
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

        // v2 surface (fork #50): `action` dispatches the verb family; v1
        // ships "send" only. Omitted action = "send" (existing callers
        // never pass it).
        if let Some(action) = input.get("action").and_then(Value::as_str) {
            if action != "send" {
                return Err(ToolError::InvalidInput(format!(
                    "action '{action}' is not available yet — v1 ships 'send' only"
                )));
            }
        }

        // Failsafe default (fork #13): an unset interrupt must not derail a
        // session that is mid-turn. v2 (fork #50) re-expresses the knob as
        // the delivery policy — the alias keeps old prompts working.
        let interrupt = resolve_mode(
            input
                .get("delivery")
                .and_then(|d| d.get("mode"))
                .and_then(Value::as_str),
            input.get("interrupt").and_then(Value::as_bool),
        )?;

        use crate::brain::agent::service::session_routes::{Delivery, deliver_to_session};

        match deliver_to_session(target, msg, interrupt) {
            Delivery::Delivered => Ok(verdict(
                true,
                "delivered",
                format!(
                    "Delivered to session {target}. It will process the message on its next turn."
                ),
                &[("notify_target", target.to_string())],
            )),
            // Queued, not lost: the target belongs to a channel that has not
            // claimed it since the last restart (#1206). Reporting this as a
            // failure would be the opposite of what happened.
            Delivery::Parked => Ok(verdict(
                true,
                "queued",
                format!(
                    "Queued for session {target}. Its channel has not claimed it since the last \
                     restart, so it will be delivered as soon as that channel next binds the \
                     session."
                ),
                &[
                    ("notify_target", target.to_string()),
                    ("notify_reason", "awaiting_channel_claim".into()),
                ],
            )),
            Delivery::RefusedInFlight { redirected_to } => {
                let who = match redirected_to {
                    Some(to) => format!(
                        "{to} (mid-turn — the message was redirected there because \
                         {target} no longer owns its channel)"
                    ),
                    None => target.to_string(),
                };
                let mut extra = vec![
                    ("notify_target", target.to_string()),
                    ("notify_reason", "mid_turn".to_string()),
                ];
                if let Some(to) = redirected_to {
                    extra.push(("notify_redirected_to", to.to_string()));
                }
                Ok(verdict(
                    false,
                    "refused",
                    format!(
                        "Refused: session {who} is mid-turn (a turn is streaming) and interrupt \
                         was not set — delivering now would derail its current task. Retry when \
                         the session goes idle, or resend with interrupt=true to queue the \
                         message for its in-flight turn's next tool-loop boundary."
                    ),
                    &extra,
                ))
            }
            Delivery::NoRoute => Ok(verdict(
                false,
                "refused",
                format!(
                    "No live route for session {target} in this process — it has not messaged \
                     since boot, or belongs to another instance/profile. Use a2a_send for \
                     cross-instance targets."
                ),
                &[
                    ("notify_target", target.to_string()),
                    ("notify_reason", "no_route".to_string()),
                ],
            )),
            // The target no longer owns its channel (fork #17): the message
            // was redirected to the session that owns it NOW (fork #19) — a
            // success, not a refusal, and the reply names where it went.
            Delivery::Redirected { to } => Ok(verdict(
                true,
                "redirected",
                redirect_message(target, to),
                &[
                    ("notify_target", target.to_string()),
                    ("notify_occupant", to.to_string()),
                ],
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdict_carries_state_and_extra_metadata() {
        let result = verdict(
            true,
            "redirected",
            "detail".into(),
            &[
                ("notify_target", "t".into()),
                ("notify_occupant", "o".into()),
            ],
        );
        assert!(result.success);
        assert_eq!(result.metadata.get("notify_state").unwrap(), "redirected");
        assert_eq!(result.metadata.get("notify_target").unwrap(), "t");
        assert_eq!(result.metadata.get("notify_occupant").unwrap(), "o");
    }

    #[test]
    fn verdict_refusal_is_error_with_state() {
        let result = verdict(
            false,
            "refused",
            "detail".into(),
            &[("notify_reason", "mid_turn".into())],
        );
        assert!(!result.success);
        assert_eq!(result.metadata.get("notify_state").unwrap(), "refused");
        assert_eq!(result.metadata.get("notify_reason").unwrap(), "mid_turn");
    }

    #[test]
    fn verdict_states_mirror_delivery_enum() {
        // The v2 external vocabulary (fork #50): every internal Delivery
        // variant maps to exactly one of these states.
        const STATES: [&str; 4] = ["delivered", "queued", "redirected", "refused"];
        for state in STATES {
            let result = verdict(true, state, "d".into(), &[]);
            assert_eq!(result.metadata.get("notify_state").unwrap(), state);
        }
    }

    #[test]
    fn resolve_mode_defaults_and_alias() {
        // Default: refuse while streaming (the fork #13 failsafe).
        assert!(resolve_mode(None, None).unwrap() == false);
        // The interrupt alias maps exactly onto the modes.
        assert!(resolve_mode(None, Some(true)).unwrap());
        assert!(resolve_mode(None, Some(false)).unwrap() == false);
        assert!(resolve_mode(Some("now"), None).unwrap() == false);
        assert!(resolve_mode(Some("turn-end"), None).unwrap());
    }

    #[test]
    fn resolve_mode_agreeing_pair_passes_disagreement_rejected() {
        assert!(resolve_mode(Some("turn-end"), Some(true)).unwrap());
        assert!(resolve_mode(Some("now"), Some(false)).unwrap() == false);
        assert!(resolve_mode(Some("now"), Some(true)).is_err());
        assert!(resolve_mode(Some("turn-end"), Some(false)).is_err());
    }

    #[test]
    fn resolve_mode_rejects_modes_before_their_engine_lands() {
        let err = resolve_mode(Some("quiet"), None).unwrap_err().to_string();
        assert!(err.contains("not available yet"), "got: {err}");
    }
}
