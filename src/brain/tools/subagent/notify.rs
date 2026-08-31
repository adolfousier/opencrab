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
use std::time::Duration;

/// Resolved v2 delivery policy (fork #50).
#[derive(Debug)]
enum DeliveryMode {
    /// Refuse while the target is mid-turn (the failsafe default).
    Now,
    /// Queue for the target's next tool-loop boundary (alias: interrupt=true).
    TurnEnd,
    /// Defer until the target has been quiet for `quiet_for`; `max_delay`
    /// forces delivery into a busy turn (fork #43/#50).
    Quiet {
        quiet_for: Duration,
        max_delay: Duration,
    },
}

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
/// agree; a disagreement is an error, never a silent precedence. `quiet`
/// defers until the target has been idle for `quiet_for_secs` (starvation
/// cap `max_delay_secs` forces delivery into a busy turn).
fn resolve_mode(
    mode: Option<&str>,
    interrupt: Option<bool>,
    delivery: Option<&Value>,
) -> Result<DeliveryMode> {
    fn secs(parent: Option<&Value>, key: &str, default: u64) -> Result<Duration> {
        match parent.and_then(|d| d.get(key)) {
            None => Ok(Duration::from_secs(default)),
            Some(v) => {
                let n = v.as_u64().ok_or_else(|| {
                    ToolError::InvalidInput(format!(
                        "delivery.{key} must be a non-negative integer"
                    ))
                })?;
                Ok(Duration::from_secs(n))
            }
        }
    }
    let resolved = match mode {
        None => None,
        Some(known @ ("now" | "turn-end")) => Some(known),
        Some("quiet") => {
            // quiet contradicts interrupt=true by definition: quiet WAITS,
            // turn-end DERAILS. interrupt=false/unset is the natural form.
            if interrupt == Some(true) {
                return Err(ToolError::InvalidInput(
                    "delivery.mode 'quiet' and interrupt=true disagree — quiet defers, \
                     interrupt derails"
                        .into(),
                ));
            }
            let quiet_for = secs(delivery, "quiet_for_secs", 60)?;
            let max_delay = secs(delivery, "max_delay_secs", 1800)?;
            return Ok(DeliveryMode::Quiet {
                quiet_for,
                max_delay,
            });
        }
        Some(other) => {
            return Err(ToolError::InvalidInput(format!(
                "delivery.mode '{other}' is not available yet — use 'now', 'turn-end' or 'quiet'"
            )));
        }
    };
    match (resolved, interrupt) {
        (Some("turn-end"), None | Some(true)) | (None, Some(true)) => Ok(DeliveryMode::TurnEnd),
        (Some("now"), None | Some(false)) | (None, None | Some(false)) => Ok(DeliveryMode::Now),
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
                            "enum": ["now", "turn-end", "quiet"],
                            "description": "'now' (default): deliver immediately; REFUSES while the target is mid-turn. 'turn-end': queue the message for the target's next tool-loop boundary even while it streams. 'quiet': defer until the target has been idle for quiet_for_secs (any turn activity restarts the clock; max_delay_secs forces delivery into a busy turn so the notice cannot be starved forever); returns a deferred verdict with a notification id."
                        },
                        "quiet_for_secs": {
                            "type": "integer",
                            "description": "quiet mode only: idle window before delivery (default 60)."
                        },
                        "max_delay_secs": {
                            "type": "integer",
                            "description": "quiet mode only: starvation cap — deliver at latest this long after acceptance, even mid-turn (default 1800)."
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
        if let Some(action) = input.get("action").and_then(Value::as_str)
            && action != "send"
        {
            return Err(ToolError::InvalidInput(format!(
                "action '{action}' is not available yet — v1 ships 'send' only"
            )));
        }

        // Failsafe default (fork #13): an unset interrupt must not derail a
        // session that is mid-turn. v2 (fork #50) re-expresses the knob as
        // the delivery policy — the alias keeps old prompts working.
        let delivery_obj = input.get("delivery");
        let mode = resolve_mode(
            delivery_obj
                .and_then(|d| d.get("mode"))
                .and_then(Value::as_str),
            input.get("interrupt").and_then(Value::as_bool),
            delivery_obj,
        )?;

        use crate::brain::agent::service::quiet_delivery;
        use crate::brain::agent::service::session_routes::{Delivery, deliver_to_session};

        // Quiet mode (fork #43/#50): bank the notice, return the id. The
        // deferred verdict is success-by-contract — accepted, not yet
        // delivered; the id is the cancel/list handle from birth.
        if let DeliveryMode::Quiet {
            quiet_for,
            max_delay,
        } = mode
        {
            let id = quiet_delivery::defer_quiet(target, msg, quiet_for, max_delay);
            return Ok(verdict(
                true,
                "deferred",
                format!(
                    "Deferred for session {target}: it will deliver once the session has been \
                     quiet for {}s (hard cap {}s). Notification id {id}.",
                    quiet_for.as_secs(),
                    max_delay.as_secs()
                ),
                &[
                    ("notify_target", target.to_string()),
                    ("notify_id", id.to_string()),
                    ("notify_reason", "quiet_window".into()),
                ],
            ));
        }

        let interrupt = matches!(mode, DeliveryMode::TurnEnd);

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
        use DeliveryMode::*;
        // Default: refuse while streaming (the fork #13 failsafe).
        assert!(matches!(resolve_mode(None, None, None).unwrap(), Now));
        // The interrupt alias maps exactly onto the modes.
        assert!(matches!(
            resolve_mode(None, Some(true), None).unwrap(),
            TurnEnd
        ));
        assert!(matches!(
            resolve_mode(None, Some(false), None).unwrap(),
            Now
        ));
        assert!(matches!(
            resolve_mode(Some("now"), None, None).unwrap(),
            Now
        ));
        assert!(matches!(
            resolve_mode(Some("turn-end"), None, None).unwrap(),
            TurnEnd
        ));
    }

    #[test]
    fn resolve_mode_agreeing_pair_passes_disagreement_rejected() {
        use DeliveryMode::*;
        assert!(matches!(
            resolve_mode(Some("turn-end"), Some(true), None).unwrap(),
            TurnEnd
        ));
        assert!(matches!(
            resolve_mode(Some("now"), Some(false), None).unwrap(),
            Now
        ));
        assert!(resolve_mode(Some("now"), Some(true), None).is_err());
        assert!(resolve_mode(Some("turn-end"), Some(false), None).is_err());
    }

    #[test]
    fn resolve_mode_rejects_unknown_modes() {
        let err = resolve_mode(Some("warp"), None, None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("not available yet"), "got: {err}");
    }

    #[test]
    fn resolve_mode_quiet_defaults_and_custom_windows() {
        use DeliveryMode::*;
        let default = resolve_mode(Some("quiet"), None, None).unwrap();
        assert!(
            matches!(default, Quiet { quiet_for, max_delay }
                if quiet_for == Duration::from_secs(60) && max_delay == Duration::from_secs(1800)),
            "got: {default:?}"
        );
        let custom = serde_json::json!({ "quiet_for_secs": 300, "max_delay_secs": 60 });
        let got = resolve_mode(Some("quiet"), None, Some(&custom)).unwrap();
        assert!(
            matches!(got, Quiet { quiet_for, max_delay }
                if quiet_for == Duration::from_secs(300) && max_delay == Duration::from_secs(60)),
            "got: {got:?}"
        );
        // Non-integer window is rejected, not silently defaulted.
        let bad = serde_json::json!({ "quiet_for_secs": "soon" });
        assert!(resolve_mode(Some("quiet"), None, Some(&bad)).is_err());
    }

    #[test]
    fn resolve_mode_quiet_contradicts_interrupt_true() {
        // quiet WAITS; interrupt=true DERAILS — passing both is an error,
        // never a silent precedence.
        assert!(resolve_mode(Some("quiet"), Some(true), None).is_err());
        // interrupt=false is the natural form and passes.
        assert!(resolve_mode(Some("quiet"), Some(false), None).is_ok());
    }
}
