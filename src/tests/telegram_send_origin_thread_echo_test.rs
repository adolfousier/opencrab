//! Tests for the #127 telegram_send addressing semantics:
//!   1. SAME-ORIGIN thread inheritance — the session-origin topic is
//!      applied ONLY when the resolved target chat IS the session-origin
//!      chat. Cross-chat sends with omitted `thread_id` must NOT inherit
//!      a topic from a chat they are not going to (the #116 poisoning
//!      chain: group topic 7198 applied to owner DM → thread not found).
//!   2. Landing echo — `landing_echo` names the resolved destination,
//!      degrading gracefully when no DB pool exists.

use crate::brain::tools::telegram_send::{landing_echo, resolve_thread_id};
use crate::channels::telegram::TelegramState;
use serde_json::json;
use teloxide::types::{MessageId, ThreadId};
use uuid::Uuid;

#[tokio::test]
async fn session_origin_topic_applies_to_same_origin_chat() {
    // The core #450 behavior is PRESERVED: omitted thread_id on a send to
    // the session's own chat still routes to the origin topic.
    let state = TelegramState::new();
    let session_id = Uuid::from_u128(0xABCD);
    state
        .register_session_chat(session_id, 12345, Some(42))
        .await;

    let input = json!({});
    let result = resolve_thread_id(&input, 12345, session_id, &state).await;
    assert_eq!(result, Some(ThreadId(MessageId(42))));
}

#[tokio::test]
async fn session_origin_topic_does_not_leak_into_other_chat() {
    // #127: session lives in chat 12345 topic 42, but the send targets a
    // DIFFERENT chat (-100999). The origin topic must NOT be applied —
    // topic 42 does not exist in that chat, and putting it on the wire
    // hard-fails with "message thread not found" (#116). The resolver
    // falls through to the chat-wide lookup (None here: no global pool).
    let state = TelegramState::new();
    let session_id = Uuid::from_u128(0xABCD);
    state
        .register_session_chat(session_id, 12345, Some(42))
        .await;

    let input = json!({});
    let result = resolve_thread_id(&input, -100999, session_id, &state).await;
    assert_eq!(
        result, None,
        "origin topic 42 must not leak into chat -100999"
    );
}

#[tokio::test]
async fn cross_chat_send_with_explicit_thread_id_still_honours_override() {
    // Explicit routing wins regardless of the chat (#450 semantics): the
    // same-chat override test covers chat==origin; here the override goes
    // to a different chat and is returned verbatim.
    let state = TelegramState::new();
    let session_id = Uuid::from_u128(0xABCD);
    state
        .register_session_chat(session_id, 12345, Some(42))
        .await;

    let input = json!({ "thread_id": 7 });
    let result = resolve_thread_id(&input, -100999, session_id, &state).await;
    assert_eq!(result, Some(ThreadId(MessageId(7))));
}

#[tokio::test]
async fn session_bound_to_general_other_chat_falls_through_to_lookup() {
    // A session bound to General (topic None) sending to ANOTHER chat:
    // no same-origin topic exists to apply, so the chat-wide lookup path
    // is taken (None in tests without a global pool). Mirrors #1319's
    // reasoning — a known General address is same-origin only now.
    let state = TelegramState::new();
    let session_id = Uuid::from_u128(0xABCD);
    state.register_session_chat(session_id, 12345, None).await;

    let input = json!({});
    let result = resolve_thread_id(&input, -100999, session_id, &state).await;
    assert_eq!(result, None);
}

#[tokio::test]
async fn session_bound_to_general_same_chat_is_known_no_thread() {
    // #1319 preserved for the SAME-origin chat: General-bound session +
    // same chat → None is a KNOWN address (no thread), not a fall-through
    // that would post into whichever topic spoke last.
    let state = TelegramState::new();
    let session_id = Uuid::from_u128(0xABCD);
    state.register_session_chat(session_id, 12345, None).await;

    let input = json!({});
    let result = resolve_thread_id(&input, 12345, session_id, &state).await;
    assert_eq!(result, None);
}

#[tokio::test]
async fn landing_echo_names_chat_and_no_topic_without_thread() {
    // No DB pool in unit tests → thread_name degrades; the echo must still
    // name the chat and the absence of a topic.
    let echo = landing_echo(-100123, None).await;
    assert!(
        echo.contains("chat -100123"),
        "echo must name the landing chat: {echo}"
    );
    assert!(
        echo.contains("no topic"),
        "unthreaded landing must say so explicitly: {echo}"
    );
}

#[tokio::test]
async fn landing_echo_degrades_to_numeric_topic_without_db() {
    // Thread present but no DB pool: numeric id, no name, no panic.
    let echo = landing_echo(-100123, Some(ThreadId(MessageId(42)))).await;
    assert!(
        echo.contains("chat -100123"),
        "echo must name the landing chat: {echo}"
    );
    assert!(
        echo.contains("topic 42"),
        "echo must carry the numeric topic: {echo}"
    );
}
