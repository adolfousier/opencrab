//! Tests for the typed target resolvers introduced by #1080 — the single
//! seam every `telegram_send` action must pass through to obtain its
//! destination. The resolvers fold chat fallback and thread precedence into
//! one call so an arm cannot take the chat and skip the topic decision
//! (the exact shape of #1079, where six arms built requests without thread
//! routing).
//!
//! These pin the resolvers' contracts directly; the eleven
//! `telegram_send_thread_id_override_test` tests keep covering the
//! precedence internals of `resolve_thread_id` underneath.

use crate::brain::tools::telegram_send::{
    resolve_chat_target, resolve_existing_target, resolve_new_target,
};
use crate::channels::telegram::TelegramState;
use serde_json::json;
use teloxide::types::{MessageId, ThreadId};
use uuid::Uuid;

// Fresh state + nil session: no session-origin chat/topic bound, no owner
// chat, no global DB pool — the resolver's fallbacks all bottom out, so
// only the explicit input fields can produce values.
fn empty_state() -> TelegramState {
    TelegramState::new()
}

fn error_text(r: crate::brain::tools::r#trait::ToolResult) -> String {
    assert!(!r.success, "expected an error ToolResult");
    r.error.unwrap_or_default()
}

#[tokio::test]
async fn new_target_honours_explicit_chat_and_thread() {
    // Explicit `chat_id` + explicit `thread_id`: both used verbatim — the
    // cron-posting shape ("release notes in #announcements").
    let input = json!({ "chat_id": 5, "thread_id": 17 });
    let target = resolve_new_target(&input, Uuid::nil(), &empty_state())
        .await
        .expect("explicit chat must resolve");
    assert_eq!(target.chat_id, 5);
    assert_eq!(target.thread_id, Some(ThreadId(MessageId(17))));
}

#[tokio::test]
async fn new_target_inherits_session_origin_topic() {
    // No explicit thread_id: the resolver inherits the topic this session
    // started in (#450), so a reply routes back to its own topic.
    let state = empty_state();
    let session_id = Uuid::from_u128(0xABCD);
    state
        .register_session_chat(session_id, 12345, Some(42))
        .await;

    let input = json!({ "chat_id": 12345 });
    let target = resolve_new_target(&input, session_id, &state)
        .await
        .expect("session chat must resolve");
    assert_eq!(target.chat_id, 12345);
    assert_eq!(target.thread_id, Some(ThreadId(MessageId(42))));
}

#[tokio::test]
async fn new_target_without_any_thread_source_is_none() {
    // Explicit chat, nil session, empty state, no DB pool: no thread source
    // exists, so the topic is honestly None (plain non-forum send).
    let input = json!({ "chat_id": 9 });
    let target = resolve_new_target(&input, Uuid::nil(), &empty_state())
        .await
        .expect("explicit chat must resolve");
    assert_eq!(target.chat_id, 9);
    assert_eq!(target.thread_id, None);
}

#[tokio::test]
async fn new_target_without_a_chat_errors() {
    // No chat_id, no session chat, no owner chat: the resolver surfaces
    // chat_or_err's guidance instead of inventing a destination.
    let input = json!({ "thread_id": 17 });
    let err = resolve_new_target(&input, Uuid::nil(), &empty_state())
        .await
        .expect_err("no chat source must error");
    let text = error_text(err);
    assert!(
        text.contains("chat_id"),
        "error should point at the missing chat: {text}"
    );
}

#[tokio::test]
async fn existing_target_resolves_chat_and_message_id() {
    let input = json!({ "chat_id": 5, "message_id": 77 });
    let target = resolve_existing_target(&input, Uuid::nil(), &empty_state())
        .await
        .expect("both fields present must resolve");
    assert_eq!(target.chat_id, 5);
    assert_eq!(target.message_id, 77);
}

#[tokio::test]
async fn existing_target_requires_message_id() {
    let input = json!({ "chat_id": 5 });
    let err = resolve_existing_target(&input, Uuid::nil(), &empty_state())
        .await
        .expect_err("missing message_id must error");
    let text = error_text(err);
    assert!(
        text.contains("'message_id'"),
        "error should name the missing param: {text}"
    );
}

#[tokio::test]
async fn existing_target_chat_error_precedes_message_id_error() {
    // Precedence pin: with BOTH chat and message_id missing, the chat error
    // wins — the same order the edit/delete/pin arms had before extraction.
    let input = json!({});
    let err = resolve_existing_target(&input, Uuid::nil(), &empty_state())
        .await
        .expect_err("no chat source must error");
    let text = error_text(err);
    assert!(
        text.contains("chat_id") && !text.contains("'message_id'"),
        "chat error must come first, got: {text}"
    );
}

#[tokio::test]
async fn chat_target_uses_explicit_chat() {
    let input = json!({ "chat_id": 3 });
    let target = resolve_chat_target(&input, Uuid::nil(), &empty_state())
        .await
        .expect("explicit chat must resolve");
    assert_eq!(target.chat_id, 3);
}

#[tokio::test]
async fn chat_target_without_a_chat_errors() {
    let input = json!({});
    let err = resolve_chat_target(&input, Uuid::nil(), &empty_state())
        .await
        .expect_err("no chat source must error");
    assert!(
        error_text(err).contains("chat_id"),
        "error should point at the missing chat"
    );
}
