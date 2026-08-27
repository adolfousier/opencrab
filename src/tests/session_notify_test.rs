//! `session_notify` surface and delivery reporting (PR #1207, issue #1203).
//!
//! The tool is a thin wrapper over `deliver_to_session`, so what matters here
//! is its contract: the schema it advertises, and that it reports a PARKED
//! delivery as queued rather than as a missing route. Parking arrived with
//! #1206, after this tool was written against a two-state bool.

use uuid::Uuid;

use crate::brain::agent::QueuedUserMessage;
use crate::brain::agent::service::restart_recovery::{expect_channel_route, test_guard};
use crate::brain::agent::service::session_routes::{Delivery, deliver_to_session, register_turn_probe};
use crate::brain::tools::subagent::SessionNotifyTool;
use crate::brain::tools::r#trait::Tool;

fn msg() -> QueuedUserMessage {
    QueuedUserMessage {
        context_text: "[session-notify from=x]\n\nbody".to_string(),
        display_text: "notify".to_string(),
        origin: crate::brain::agent::PushOrigin::Other,
    }
}

#[tokio::test]
// The guard serializes suites touching the process-global parked-queue state
// (#1206); holding it across the tool `.await` below is the entire point —
// the awaited region must not interleave with another test's park.
#[allow(clippy::await_holding_lock)]
async fn test_notify_pushes_carry_sessionnotify_origin_for_topic_echo() {
    // #1221 notify lane: the Telegram resume callback echoes only origins it
    // knows about, so the tool must tag its pushes SessionNotify — the silent
    // Other default keeps every session_notify push invisible in topics.
    let _guard = test_guard();
    let session = Uuid::new_v4();
    let captured: std::sync::Arc<std::sync::Mutex<Option<QueuedUserMessage>>> =
        std::sync::Arc::new(std::sync::Mutex::new(None));
    let sink = captured.clone();
    crate::brain::agent::service::session_routes::register_session_route(
        session,
        std::sync::Arc::new(move |_id, queued| {
            *sink.lock().unwrap() = Some(queued);
        }),
    );

    let context = crate::brain::tools::r#trait::ToolExecutionContext::new(Uuid::new_v4());
    let outcome = SessionNotifyTool
        .execute(
            serde_json::json!({"target_session": session.to_string(), "message": "ping"}),
            &context,
        )
        .await;
    assert!(outcome.is_ok(), "delivery should succeed: {outcome:?}");
    let queued = captured.lock().unwrap().take().expect("message enqueued");
    assert_eq!(
        queued.origin,
        crate::brain::agent::PushOrigin::SessionNotify,
        "#1221: Other-tagged notify pushes never earn an echo bubble"
    );
}

#[test]
fn test_schema_requires_target_and_message() {
    let tool = SessionNotifyTool;
    assert_eq!(tool.name(), "session_notify");
    assert!(!tool.requires_approval());
    assert!(!tool.description().is_empty());

    let schema = tool.input_schema();
    let required: Vec<&str> = schema["required"]
        .as_array()
        .expect("required list")
        .iter()
        .map(|v| v.as_str().expect("required entry is a string"))
        .collect();
    assert_eq!(required, vec!["target_session", "message"]);
    assert!(schema["properties"]["target_session"].is_object());
    assert!(schema["properties"]["message"].is_object());
}

#[test]
fn test_a_parked_delivery_is_not_a_missing_route() {
    // The distinction the tool reports on: a session whose channel has not
    // claimed it since a restart holds the message rather than losing it.
    let _guard = test_guard();
    let session = Uuid::new_v4();

    expect_channel_route(session);

    let outcome = deliver_to_session(session, msg(), false);
    assert_eq!(
        outcome,
        Delivery::Parked,
        "#1206: a park is queued, not lost — reporting it as a missing route \
         tells the caller the opposite of what happened"
    );
}

#[test]
fn test_an_unroutable_session_is_reported_as_such() {
    let _guard = test_guard();
    // No local route is registered in tests, so nothing can take it.
    assert_eq!(
        deliver_to_session(Uuid::new_v4(), msg(), false),
        Delivery::NoRoute
    );
}

// ── In-flight gate (fork #13) ────────────────────────────────────────────

#[test]
fn test_inflight_target_refuses_without_interrupt() {
    let _guard = test_guard();
    let session = Uuid::new_v4();
    expect_channel_route(session);
    register_turn_probe(session, std::sync::Arc::new(|| true));
    assert_eq!(
        deliver_to_session(session, msg(), false),
        Delivery::RefusedInFlight,
        "#13: default-false must refuse a mid-turn target, not derail it"
    );
}

#[test]
fn test_interrupt_true_delivers_to_inflight_target() {
    let _guard = test_guard();
    let session = Uuid::new_v4();
    expect_channel_route(session);
    register_turn_probe(session, std::sync::Arc::new(|| true));
    // Parked, not delivered: the channel route is expected but unclaimed, so
    // the point here is only that the gate let the message THROUGH.
    assert_eq!(
        deliver_to_session(session, msg(), true),
        Delivery::Parked,
        "#13: interrupt=true rides today's queue-for-boundary semantics"
    );
}

#[test]
fn test_idle_target_delivers_without_interrupt() {
    let _guard = test_guard();
    let session = Uuid::new_v4();
    expect_channel_route(session);
    register_turn_probe(session, std::sync::Arc::new(|| false));
    assert_eq!(
        deliver_to_session(session, msg(), false),
        Delivery::Parked,
        "#13: idle target — the gate never engages"
    );
}

#[test]
fn test_no_probe_fails_open() {
    let _guard = test_guard();
    let session = Uuid::new_v4();
    expect_channel_route(session);
    // No probe registered: a surface without turn state must stay notifyable.
    assert_eq!(
        deliver_to_session(session, msg(), false),
        Delivery::Parked,
        "#13: unknown turn state fails open — never refuse on missing semantics"
    );
}

#[tokio::test]
async fn test_tool_reports_refusal_with_remedy() {
    let _guard = test_guard();
    let session = Uuid::new_v4();
    expect_channel_route(session);
    register_turn_probe(session, std::sync::Arc::new(|| true));

    let context = crate::brain::tools::r#trait::ToolExecutionContext::new(Uuid::new_v4());
    let outcome = SessionNotifyTool
        .execute(
            serde_json::json!({"target_session": session.to_string(), "message": "ping"}),
            &context,
        )
        .await;
    let result = outcome.expect("tool executes");
    assert!(!result.success, "#13: refusal must read as failure to the sender");
    let error = result.error.expect("#13: refusal carries an explanation");
    assert!(
        error.contains("interrupt=true"),
        "#13: the error must name the remedy so the sender learns the knob: {error}"
    );
}

#[tokio::test]
async fn test_tool_interrupt_param_reaches_delivery() {
    let _guard = test_guard();
    let session = Uuid::new_v4();
    let captured: std::sync::Arc<
        std::sync::Mutex<Option<QueuedUserMessage>>,
    > = std::sync::Arc::new(std::sync::Mutex::new(None));
    let sink = captured.clone();
    crate::brain::agent::service::session_routes::register_session_route(
        session,
        std::sync::Arc::new(move |_id, queued| {
            *sink.lock().unwrap() = Some(queued);
        }),
    );
    register_turn_probe(session, std::sync::Arc::new(|| true));

    let context = crate::brain::tools::r#trait::ToolExecutionContext::new(Uuid::new_v4());
    let outcome = SessionNotifyTool
        .execute(
            serde_json::json!({
                "target_session": session.to_string(),
                "message": "ping",
                "interrupt": true
            }),
            &context,
        )
        .await;
    assert!(outcome.expect("tool executes").success, "interrupt=true must deliver");
    let queued = captured.lock().unwrap().take().expect("message enqueued");
    assert!(
        !queued.context_text.contains("queued while you were working"),
        "framing is added by the channel queue branch, not by the tool"
    );
}
