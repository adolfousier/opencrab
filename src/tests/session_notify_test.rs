//! `session_notify` surface and delivery reporting (PR #1207, issue #1203).
//!
//! The tool is a thin wrapper over `deliver_to_session`, so what matters here
//! is its contract: the schema it advertises, and that it reports a PARKED
//! delivery as queued rather than as a missing route. Parking arrived with
//! #1206, after this tool was written against a two-state bool.

use uuid::Uuid;

use crate::brain::agent::QueuedUserMessage;
use crate::brain::agent::service::restart_recovery::{expect_channel_route, test_guard};
use crate::brain::agent::service::session_routes::{
    ChannelOwnership, Delivery, deliver_to_session, register_channel_owner_probe,
    register_turn_probe,
};
use crate::brain::tools::subagent::SessionNotifyTool;
use crate::brain::tools::r#trait::Tool;

fn msg() -> QueuedUserMessage {
    QueuedUserMessage {
        context_text: "[session-notify from=x]\n\nbody".to_string(),
        display_text: "notify".to_string(),
        origin: crate::brain::agent::PushOrigin::Other,
        bg_meta: None,
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

// ── Channel-ownership gate (fork #17) ────────────────────────────────────
//
// A session REPLACED on its chat/topic (idle-timeout reset creates a
// successor) keeps its delivery route — routes are UUID-keyed and never
// evicted. Without the gate any push wakes the replaced session into the
// successor's conversation. The gate is the OUTERMOST check: it guards who
// owns the channel, not the target's turn state, so `interrupt` does not
// override it. Unknown ownership fails open, same posture as the turn gate.

#[test]
fn test_occupied_channel_refuses_delivery() {
    let _guard = test_guard();
    let session = Uuid::new_v4();
    let occupant = Uuid::new_v4();
    register_channel_owner_probe(
        session,
        std::sync::Arc::new(move || ChannelOwnership::Occupied { occupant }),
    );
    assert_eq!(
        deliver_to_session(session, msg(), false),
        Delivery::RefusedChannelOccupied { occupant },
        "#17: a replaced session must not be woken into its successor's channel"
    );
}

#[test]
fn test_occupied_channel_refuses_even_with_interrupt() {
    let _guard = test_guard();
    let session = Uuid::new_v4();
    let occupant = Uuid::new_v4();
    register_channel_owner_probe(
        session,
        std::sync::Arc::new(move || ChannelOwnership::Occupied { occupant }),
    );
    assert_eq!(
        deliver_to_session(session, msg(), true),
        Delivery::RefusedChannelOccupied { occupant },
        "#17: interrupt overrides the TURN gate only — never channel ownership"
    );
}

#[test]
fn test_ownership_gate_outranks_turn_gate() {
    let _guard = test_guard();
    let session = Uuid::new_v4();
    let occupant = Uuid::new_v4();
    register_turn_probe(session, std::sync::Arc::new(|| true));
    register_channel_owner_probe(
        session,
        std::sync::Arc::new(move || ChannelOwnership::Occupied { occupant }),
    );
    // Mid-turn AND replaced: the refusal must name the occupant, not the
    // turn state — redirecting to the occupant is the actionable remedy.
    assert_eq!(
        deliver_to_session(session, msg(), false),
        Delivery::RefusedChannelOccupied { occupant },
        "#17: the ownership gate runs outside the turn gate"
    );
}

#[test]
fn test_owned_channel_passes_ownership_gate() {
    let _guard = test_guard();
    let session = Uuid::new_v4();
    expect_channel_route(session);
    register_channel_owner_probe(session, std::sync::Arc::new(|| ChannelOwnership::Owned));
    assert_eq!(
        deliver_to_session(session, msg(), false),
        Delivery::Parked,
        "#17: an owning session passes the gate — parked here only because \
         the expected route is unclaimed"
    );
}

#[test]
fn test_unknown_ownership_fails_open() {
    let _guard = test_guard();
    let session = Uuid::new_v4();
    expect_channel_route(session);
    register_channel_owner_probe(session, std::sync::Arc::new(|| ChannelOwnership::Unknown));
    assert_eq!(
        deliver_to_session(session, msg(), false),
        Delivery::Parked,
        "#17: unknown ownership fails open — never refuse on missing semantics"
    );
}

#[tokio::test]
#[expect(clippy::await_holding_lock)]
async fn test_tool_reports_channel_occupied_with_occupant() {
    let _guard = test_guard();
    let session = Uuid::new_v4();
    let occupant = Uuid::new_v4();
    register_channel_owner_probe(
        session,
        std::sync::Arc::new(move || ChannelOwnership::Occupied { occupant }),
    );

    let context = crate::brain::tools::r#trait::ToolExecutionContext::new(Uuid::new_v4());
    let outcome = SessionNotifyTool
        .execute(
            serde_json::json!({"target_session": session.to_string(), "message": "ping"}),
            &context,
        )
        .await;
    let result = outcome.expect("tool executes");
    assert!(
        !result.success,
        "#17: refusal must read as failure to the sender"
    );
    let error = result.error.expect("#17: refusal carries an explanation");
    assert!(
        error.contains(&occupant.to_string()),
        "#17: the refusal must name the occupying session so the sender can \
         redirect without a discovery round: {error}"
    );
    assert!(
        error.contains("no longer owns its channel"),
        "#17: the refusal must say WHY: {error}"
    );
}

#[tokio::test]
async fn test_ownership_mirror_tracks_channel_replacement() {
    // The sync mirror behind the telegram probe (state.rs): binding a
    // successor to the same (chat, topic) must flip the old session to
    // Occupied naming the successor, while the successor reads Owned and a
    // never-bound session reads Unknown.
    use crate::channels::telegram::TelegramState;
    let state = TelegramState::new();
    let old = Uuid::new_v4();
    let successor = Uuid::new_v4();
    let (chat, topic) = (-100_123_456_i64, Some(42_i32));

    assert_eq!(
        state.channel_ownership_of(old),
        ChannelOwnership::Unknown,
        "#17: never-bound session — no binding recorded"
    );

    state.register_session_chat(old, chat, topic).await;
    assert_eq!(state.channel_ownership_of(old), ChannelOwnership::Owned);

    // Idle-timeout replacement: same channel, new session.
    state.register_session_chat(successor, chat, topic).await;
    assert_eq!(
        state.channel_ownership_of(old),
        ChannelOwnership::Occupied {
            occupant: successor
        },
        "#17: the replaced session must see its successor by name"
    );
    assert_eq!(
        state.channel_ownership_of(successor),
        ChannelOwnership::Owned,
        "#17: the successor owns the channel it just bound"
    );
}

#[tokio::test]
async fn test_ownership_mirror_keys_dm_and_general_buckets_separately() {
    // (chat, None) buckets — DMs / non-forum groups — are channels too: a
    // replacement there must occupy exactly that bucket and no other.
    use crate::channels::telegram::TelegramState;
    let state = TelegramState::new();
    let old = Uuid::new_v4();
    let successor = Uuid::new_v4();
    let chat = 777_i64;

    state.register_session_chat(old, chat, None).await;
    state.register_session_chat(successor, chat, None).await;
    assert_eq!(
        state.channel_ownership_of(old),
        ChannelOwnership::Occupied {
            occupant: successor
        }
    );
}
