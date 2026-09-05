//! `session/notify` — mechanical session notifications for tooling (#23).
//!
//! Thin JSON-RPC wrapper over `session_routes::deliver_to_session`, the SAME
//! route the agent's `session_notify` tool uses, so external tooling (the
//! `opencrabs session notify` CLI verb and, on top of it, the oc-deploy
//! post-CI fan-out — #24) can post into a live session's queue.
//!
//! Zombie-wake guard (#17 class): the target must exist as a row in the
//! sessions table before the route table is touched at all. An unknown or
//! dead uuid (NO row — never one that merely changed state) yields
//! `no_route` without ever reaching `deliver_to_session`, whose local-route
//! fallback would otherwise inject the message into this process's own boot
//! channel — traffic resurrected for a session that is gone.
//!
//! ARCHIVED sessions are NOT dead (owner directive 2026-08-28): they pass
//! the gate like any live session and auto-route exactly as everywhere else
//! — an archived session whose channel a successor now occupies is
//! REDIRECTED to the occupant by the #19 machinery, with provenance
//! framing. Existence-gate only, never an activity-gate.
//!
//! SENDER FRAMING (owner amendment 2026-08-28, "Overridable"): the CLI lane
//! has no sender session (it is a separate process), so instead of the
//! agent tool's `from=<uuid>` the header stamps `from=cli:<label>` —
//! default [`DEFAULT_CLI_SENDER_LABEL`], overridable via the `sender`
//! param (CLI: `--sender`). The telegram echo surface renders the label
//! verbatim; the recipient's model still reads the mechanical frame.

use crate::a2a::types::*;
use crate::brain::agent::service::session_routes::{Delivery, deliver_to_session};
use crate::brain::agent::{PushOrigin, QueuedUserMessage};
use crate::services::{ServiceContext, SessionService};

/// The CLI lane's prefix inside the mechanical `[session-notify from=…]`
/// header (#23). The CLI verb runs as a separate process with no sender
/// session, so it stamps `cli:<label>` instead of a uuid; the telegram echo
/// surface (`channels::telegram::resume::split_notify_header`) recognizes
/// the prefix and renders the carried label verbatim. Agent-to-agent pushes
/// keep the bare-uuid shape (#1203/#1225).
pub(crate) const CLI_SENDER_PREFIX: &str = "cli:";

/// Default sender label for CLI notifications (#23) — overridable via the
/// `sender` JSON-RPC param / the `--sender` CLI flag (owner amendment
/// 2026-08-28).
pub(crate) const DEFAULT_CLI_SENDER_LABEL: &str = "CLI tooling";

/// Cap for an overridden sender label: the label rides inside the
/// receipt-card summary line, so a pathological value must not eat the
/// preview budget.
pub(crate) const CLI_SENDER_LABEL_MAX_CHARS: usize = 64;

/// Handle a `session/notify` JSON-RPC call (#23).
///
/// Business outcomes are returned as JSON-RPC SUCCESSES carrying
/// `{outcome, detail}` — the caller maps them to exit codes. The only
/// JSON-RPC errors this method emits are protocol-level (malformed params,
/// lookup failure), never delivery results.
pub async fn handle_session_notify(
    req_id: serde_json::Value,
    params: serde_json::Value,
    service_context: ServiceContext,
) -> JsonRpcResponse {
    let session_id = match params.get("session_id").and_then(serde_json::Value::as_str) {
        Some(raw) => match raw.parse::<uuid::Uuid>() {
            Ok(id) => id,
            Err(_) => {
                return JsonRpcResponse::error(
                    req_id,
                    error_codes::INVALID_PARAMS,
                    format!("'session_id' is not a valid UUID: {raw}"),
                );
            }
        },
        None => {
            return JsonRpcResponse::error(
                req_id,
                error_codes::INVALID_PARAMS,
                "'session_id' is required",
            );
        }
    };
    let message = match params.get("message").and_then(serde_json::Value::as_str) {
        Some(m) if !m.trim().is_empty() => m.to_string(),
        _ => {
            return JsonRpcResponse::error(
                req_id,
                error_codes::INVALID_PARAMS,
                "'message' is required and must be non-empty",
            );
        }
    };
    let title = params
        .get("title")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string);
    let interrupt = params
        .get("interrupt")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    // Sender label (#23): no sender session exists for the CLI lane, so the
    // label is carried verbatim — default DEFAULT_CLI_SENDER_LABEL,
    // overridable by the caller. Validation: the label rides inside
    // `[session-notify from=cli:<label>]`, so it may not contain the closing
    // bracket or newlines, and it is capped to keep the receipt-card summary
    // readable.
    let sender = match params.get("sender").and_then(serde_json::Value::as_str) {
        Some(raw) => {
            let label = raw.trim();
            if label.is_empty() {
                DEFAULT_CLI_SENDER_LABEL.to_string()
            } else if label.contains(']') || label.contains('\n') || label.contains('\r') {
                return JsonRpcResponse::error(
                    req_id,
                    error_codes::INVALID_PARAMS,
                    "'sender' must not contain ']' or newlines",
                );
            } else if label.chars().count() > CLI_SENDER_LABEL_MAX_CHARS {
                return JsonRpcResponse::error(
                    req_id,
                    error_codes::INVALID_PARAMS,
                    format!("'sender' must be at most {CLI_SENDER_LABEL_MAX_CHARS} chars"),
                );
            } else {
                label.to_string()
            }
        }
        None => DEFAULT_CLI_SENDER_LABEL.to_string(),
    };

    // Zombie-wake guard (#23, #17 class): only a session with a DB row may
    // be notified. `deliver_to_session` is never touched for a uuid with NO
    // row — its local-route fallback would hand the message to this
    // process's own boot channel, resurrecting traffic for a session that no
    // longer exists. An ARCHIVED row passes: archived sessions auto-route
    // like anywhere else (#19 redirect to the successor occupying the
    // channel), so the gate checks existence only, never activity.
    let session_svc = SessionService::new(service_context);
    match session_svc.get_session(session_id).await {
        Ok(Some(_session)) => {}
        Ok(None) => {
            return JsonRpcResponse::success(
                req_id,
                serde_json::json!({
                    "outcome": "no_route",
                    "detail": format!(
                        "session {session_id} does not exist — nothing sent, nothing created"
                    ),
                }),
            );
        }
        Err(e) => {
            return JsonRpcResponse::error(
                req_id,
                error_codes::INTERNAL_ERROR,
                format!("session lookup failed: {e}"),
            );
        }
    }

    // Same message shape as the agent's session_notify tool
    // (tools/subagent/notify.rs): SessionNotify origin so the topic-echo
    // surface renders the push (#1221), and a mechanical sender frame. The
    // CLI lane stamps `cli:<label>` instead of a uuid — there is no sender
    // session; the echo renders the label verbatim.
    let header = match &title {
        Some(t) => format!("📨 {t} (from {sender}):"),
        None => format!("📨 notify from {sender}:"),
    };
    let msg = QueuedUserMessage {
        context_text: format!("[session-notify from={CLI_SENDER_PREFIX}{sender}]\n\n{message}"),
        display_text: format!("{header}\n{message}"),
        origin: PushOrigin::SessionNotify,
        bg_meta: None,
    };

    let (outcome, detail) = match deliver_to_session(session_id, msg, interrupt) {
        Delivery::Delivered => ("delivered", format!("delivered to session {session_id}")),
        Delivery::Redirected { to } => (
            "delivered",
            format!(
                "redirected to session {to}: session {session_id} no longer owns its \
                 channel (#19)"
            ),
        ),
        // Queued, not lost: the session's channel has not claimed it since
        // the last restart (#1206). Reporting this as a failure would be the
        // opposite of what happened — same reading as the agent tool.
        Delivery::Parked => (
            "parked",
            format!(
                "queued for session {session_id}: its channel has not claimed it since \
                 the last restart (#1206) — it delivers on the next claim"
            ),
        ),
        Delivery::RefusedInFlight { redirected_to } => {
            let who = redirected_to.map_or_else(
                || session_id.to_string(),
                |to| format!("{to} (redirected from {session_id})"),
            );
            (
                "refused_in_flight",
                format!(
                    "session {who} is mid-turn and interrupt was not set — retry when \
                     idle or resend with interrupt=true (#13 failsafe)"
                ),
            )
        }
        Delivery::NoRoute => (
            "no_route",
            format!("no live route for session {session_id} and nothing is holding it"),
        ),
    };

    JsonRpcResponse::success(
        req_id,
        serde_json::json!({ "outcome": outcome, "detail": detail }),
    )
}
