//! session/notify handler (#23): delivery through registered session routes, channel ownership, and the CLI sender prefix.

use crate::a2a::handler::notify::*;
use crate::a2a::test_helpers::helpers::placeholder_service_context;
use crate::a2a::types::*;
use crate::brain::agent::service::restart_recovery::test_guard;
use crate::brain::agent::service::session_routes::{ChannelOwnership, register_session_route};
use crate::brain::agent::{PushOrigin, QueuedUserMessage};
use crate::services::SessionService;
use std::sync::{Arc, Mutex};

fn params(session_id: &str, message: &str) -> serde_json::Value {
    serde_json::json!({ "session_id": session_id, "message": message })
}

fn outcome_of(resp: &JsonRpcResponse) -> String {
    resp.result
        .as_ref()
        .expect("success response")
        .get("outcome")
        .and_then(serde_json::Value::as_str)
        .expect("outcome field")
        .to_string()
}

#[tokio::test]
async fn dead_uuid_is_refused_without_touching_the_route_table() {
    // #23 acceptance: unknown uuid → no_route, nothing created. The DB
    // is empty, so the zombie-wake guard must fire BEFORE
    // deliver_to_session — no route is registered, and if the guard were
    // skipped the local-route fallback would still yield a non-no_route
    // outcome only when LOCAL_ROUTE is set; the DB gate makes the result
    // deterministic either way.
    let ctx = placeholder_service_context().await;
    let dead = uuid::Uuid::new_v4();
    let resp =
        handle_session_notify(serde_json::json!(1), params(&dead.to_string(), "ping"), ctx).await;
    assert!(
        resp.error.is_none(),
        "dead uuid is a business outcome, not a protocol error: {resp:?}"
    );
    assert_eq!(outcome_of(&resp), "no_route");
}

#[tokio::test]
// test_guard serializes suites touching the process-global route table;
// holding it across the delivery `.await`s below is the entire point —
// this suite's registered route must not interleave with another test's
// (#22 shape, session_notify_test precedent).
#[allow(clippy::await_holding_lock)]
async fn live_uuid_delivers_through_the_claimed_route() {
    // #23 acceptance: live uuid → delivered via the same
    // deliver_to_session path the agent tool uses.
    let _guard = test_guard();
    let ctx = placeholder_service_context().await;
    let session = SessionService::new(ctx.clone())
        .create_session(Some("#23 test session".to_string()))
        .await
        .expect("session row created");
    let sid = session.id;

    let captured: Arc<Mutex<Option<QueuedUserMessage>>> = Arc::new(Mutex::new(None));
    let sink = captured.clone();
    register_session_route(
        sid,
        Arc::new(move |_id, queued| {
            *sink.lock().unwrap() = Some(queued);
        }),
    );

    let resp =
        handle_session_notify(serde_json::json!(2), params(&sid.to_string(), "ping"), ctx).await;
    assert!(resp.error.is_none(), "{resp:?}");
    assert_eq!(outcome_of(&resp), "delivered");
    let queued = captured.lock().unwrap().take().expect("message enqueued");
    assert_eq!(queued.origin, PushOrigin::SessionNotify);
    assert!(queued.context_text.contains(&format!(
        "[session-notify from={CLI_SENDER_PREFIX}{DEFAULT_CLI_SENDER_LABEL}]"
    )));
}

#[tokio::test]
// test_guard: same serialization rationale as the suite above — the
// registered route must survive the delivery `.await`s untouched.
#[allow(clippy::await_holding_lock)]
async fn sender_override_rides_the_header() {
    // #23 owner amendment ("Overridable"): the sender label is
    // overridable via the `sender` param (CLI: `--sender`), and the
    // echo surface reads it off the cli:-prefixed header verbatim.
    let _guard = test_guard();
    let ctx = placeholder_service_context().await;
    let session = SessionService::new(ctx.clone())
        .create_session(Some("#23 sender override".to_string()))
        .await
        .expect("session row created");
    let sid = session.id;

    let captured: Arc<Mutex<Option<QueuedUserMessage>>> = Arc::new(Mutex::new(None));
    let sink = captured.clone();
    register_session_route(
        sid,
        Arc::new(move |_id, queued| {
            *sink.lock().unwrap() = Some(queued);
        }),
    );

    let mut p = params(&sid.to_string(), "ping");
    p["sender"] = serde_json::json!("oc-deploy");
    let resp = handle_session_notify(serde_json::json!(7), p, ctx).await;
    assert!(resp.error.is_none(), "{resp:?}");
    assert_eq!(outcome_of(&resp), "delivered");
    let queued = captured.lock().unwrap().take().expect("message enqueued");
    assert!(
        queued
            .context_text
            .contains("[session-notify from=cli:oc-deploy]"),
        "override must ride the cli:-prefixed header: {}",
        queued.context_text
    );
    assert!(
        queued.display_text.contains("from oc-deploy"),
        "display frame names the overridden sender: {}",
        queued.display_text
    );
}

#[tokio::test]
async fn malformed_params_are_protocol_errors() {
    let ctx = placeholder_service_context().await;
    let bad_uuid = handle_session_notify(
        serde_json::json!(3),
        params("not-a-uuid", "ping"),
        ctx.clone(),
    )
    .await;
    assert_eq!(
        bad_uuid.error.expect("error response").code,
        error_codes::INVALID_PARAMS
    );

    let empty_msg = handle_session_notify(
        serde_json::json!(4),
        params(&uuid::Uuid::new_v4().to_string(), "   "),
        ctx.clone(),
    )
    .await;
    assert_eq!(
        empty_msg.error.expect("error response").code,
        error_codes::INVALID_PARAMS
    );

    // A sender label that would break the `[session-notify from=cli:<label>]`
    // framing is a protocol error, not a delivery result.
    let mut bad_sender = params(&uuid::Uuid::new_v4().to_string(), "ping");
    bad_sender["sender"] = serde_json::json!("bad]label");
    let bad_sender_resp = handle_session_notify(serde_json::json!(5), bad_sender, ctx).await;
    assert_eq!(
        bad_sender_resp.error.expect("error response").code,
        error_codes::INVALID_PARAMS
    );
}

#[tokio::test]
// test_guard: this suite touches the route table AND the channel-owner
// registry across `.await`s (create → archive → occupy → notify); the
// guard keeps the whole sequence atomic against other suites.
#[allow(clippy::await_holding_lock)]
async fn archived_session_auto_routes_to_its_successor() {
    // Owner directive 2026-08-28: archived ≠ dead. An archived session
    // whose channel a successor occupies must auto-route exactly like
    // any session_notify — the #19 redirect carries the notification to
    // the occupant with provenance framing, never a no_route refusal.
    let _guard = test_guard();
    let ctx = placeholder_service_context().await;
    let svc = SessionService::new(ctx.clone());
    let old = svc
        .create_session(Some("#23 old session".to_string()))
        .await
        .expect("old session row");
    svc.archive_session(old.id).await.expect("archived");
    let successor = svc
        .create_session(Some("#23 successor session".to_string()))
        .await
        .expect("successor session row");

    // The old session's channel is now occupied by the successor, and the
    // successor has a live route — the exact replaced-session shape.
    let occupant = successor.id;
    crate::brain::agent::service::session_routes::register_channel_owner_probe(
        old.id,
        std::sync::Arc::new(move || ChannelOwnership::Occupied { occupant }),
    );
    let captured: Arc<Mutex<Option<QueuedUserMessage>>> = Arc::new(Mutex::new(None));
    let sink = captured.clone();
    register_session_route(
        successor.id,
        Arc::new(move |_id, queued| {
            *sink.lock().unwrap() = Some(queued);
        }),
    );

    let resp = handle_session_notify(
        serde_json::json!(5),
        params(&old.id.to_string(), "ping"),
        ctx,
    )
    .await;
    assert!(resp.error.is_none(), "{resp:?}");
    assert_eq!(outcome_of(&resp), "delivered");
    let detail = resp
        .result
        .unwrap()
        .get("detail")
        .unwrap()
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        detail.contains("redirected"),
        "detail should name the redirect: {detail}"
    );
    let queued = captured
        .lock()
        .unwrap()
        .take()
        .expect("successor received the redirect");
    assert!(
        queued
            .context_text
            .contains(&format!("originally for session {}", old.id)),
        "provenance framing must name the archived session: {}",
        queued.context_text
    );
}
