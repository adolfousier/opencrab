//! `session_notify` surface and delivery reporting (PR #1207, issue #1203).
//!
//! The tool is a thin wrapper over `deliver_to_session`, so what matters here
//! is its contract: the schema it advertises, and that it reports a PARKED
//! delivery as queued rather than as a missing route. Parking arrived with
//! #1206, after this tool was written against a two-state bool.

use uuid::Uuid;

use crate::brain::agent::QueuedUserMessage;
use crate::brain::agent::service::restart_recovery::{expect_channel_route, test_guard};
use crate::brain::agent::service::session_routes::{Delivery, deliver_to_session};
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

    let outcome = deliver_to_session(session, msg());
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
    assert_eq!(deliver_to_session(Uuid::new_v4(), msg()), Delivery::NoRoute);
}
