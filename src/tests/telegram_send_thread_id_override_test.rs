//! Tests for `telegram_send::resolve_thread_id` — the helper that
//! lets the agent pass an explicit `thread_id` to override the
//! auto-lookup behavior added in commit `1e46cd26` (closes #130).
//!
//! Use cases the override enables:
//!   * Cron jobs posting to a specific topic (e.g. daily summary in
//!     #announcements regardless of what was discussed last in #dev).
//!   * Multi-topic conversations where the agent wants to post into
//!     a topic OTHER than the most recent one stored.
//!   * Explicit user instruction: "post this in topic 17".
//!
//! Suggested by leshchenko1979 on issue #130:
//! https://github.com/adolfousier/opencrabs/issues/130#issuecomment-4582189795

use crate::brain::tools::telegram_send::resolve_thread_id;
use crate::channels::telegram::TelegramState;
use serde_json::json;
use teloxide::types::{MessageId, ThreadId};
use uuid::Uuid;

// A fresh state + nil session means no session-origin topic is bound, so the
// resolver behaves exactly like the old two-arg version (explicit override,
// else auto-lookup). Session-origin routing (#450) is covered separately.
fn empty_state() -> TelegramState {
    TelegramState::new()
}

#[tokio::test]
async fn explicit_thread_id_is_returned_verbatim() {
    let input = json!({ "thread_id": 17 });
    let result = resolve_thread_id(&input, 0, Uuid::nil(), &empty_state()).await;
    assert_eq!(result, Some(ThreadId(MessageId(17))));
}

#[tokio::test]
async fn explicit_thread_id_works_for_negative_legacy_thread_ids() {
    // Legacy Telegram chat shapes occasionally surface negative
    // thread_id values within i32 range. The helper must pass them
    // through, not reject them.
    let input = json!({ "thread_id": -2147483 });
    let result = resolve_thread_id(&input, 0, Uuid::nil(), &empty_state()).await;
    assert_eq!(result, Some(ThreadId(MessageId(-2147483))));
}

#[tokio::test]
async fn explicit_thread_id_overflowing_i32_falls_back_to_lookup() {
    // teloxide's ThreadId wraps MessageId(i32). Values past i32::MAX
    // can't be represented. Rather than returning a wrong/zero ID,
    // the helper falls through to the auto-lookup path.
    let input = json!({ "thread_id": 9_999_999_999_i64 });
    // No global pool initialised → auto-lookup returns None → final
    // result is None. We just confirm no panic + no garbage value.
    let result = resolve_thread_id(&input, 12345, Uuid::nil(), &empty_state()).await;
    assert_eq!(result, None);
}

#[tokio::test]
async fn no_explicit_thread_id_falls_back_to_lookup() {
    // Absent field → auto-lookup path. In tests the global pool
    // isn't initialised so the lookup returns None; the important
    // contract is "no override path, no garbage value, no panic".
    let input = json!({ "chat_id": 12345 });
    let result = resolve_thread_id(&input, 12345, Uuid::nil(), &empty_state()).await;
    assert_eq!(result, None);
}

#[tokio::test]
async fn numeric_string_thread_id_is_coerced() {
    // #646: models frequently emit numeric args as JSON strings.
    // A parseable string override is coerced (trim + parse) rather
    // than silently discarded — rejecting it forced lookup fallbacks
    // for perfectly valid overrides.
    let input = json!({ "thread_id": "17" });
    let result = resolve_thread_id(&input, 12345, Uuid::nil(), &empty_state()).await;
    assert_eq!(result, Some(ThreadId(MessageId(17))));
}

#[tokio::test]
async fn truly_non_integer_thread_id_falls_back_to_lookup() {
    // Defensive: genuinely malformed values ("abc", objects, arrays)
    // still fall back to auto-lookup so a malformed override can't
    // poison routing. Only cleanly-parseable numerics coerce (#646).
    let input = json!({ "thread_id": "abc" });
    let result = resolve_thread_id(&input, 12345, Uuid::nil(), &empty_state()).await;
    // Auto-lookup returns None in test (no global pool).
    assert_eq!(result, None);
}

#[tokio::test]
async fn explicit_thread_id_zero_is_returned() {
    // Telegram's General topic is sometimes represented as thread 0
    // depending on API surface. The helper doesn't second-guess
    // i32-valid values.
    let input = json!({ "thread_id": 0 });
    let result = resolve_thread_id(&input, 0, Uuid::nil(), &empty_state()).await;
    assert_eq!(result, Some(ThreadId(MessageId(0))));
}

#[tokio::test]
async fn session_origin_topic_is_used_when_no_explicit_thread_id() {
    // #450: with no explicit thread_id, the resolver inherits the forum topic
    // this session started in (the same session_topic map the interactive-question tool
    // uses), so a reply routes back to the originating topic automatically.
    let state = empty_state();
    let session_id = Uuid::from_u128(0xABCD);
    state
        .register_session_chat(session_id, 12345, Some(42))
        .await;

    let input = json!({});
    let result = resolve_thread_id(&input, 12345, session_id, &state).await;
    assert_eq!(result, Some(ThreadId(MessageId(42))));
}

#[tokio::test]
async fn explicit_thread_id_overrides_session_origin_topic() {
    // Explicit routing still wins over the inherited session topic (#450).
    let state = empty_state();
    let session_id = Uuid::from_u128(0xABCD);
    state
        .register_session_chat(session_id, 12345, Some(42))
        .await;

    let input = json!({ "thread_id": 7 });
    let result = resolve_thread_id(&input, 12345, session_id, &state).await;
    assert_eq!(result, Some(ThreadId(MessageId(7))));
}

#[tokio::test]
async fn non_forum_session_origin_falls_through_to_lookup() {
    // A session bound to a non-forum chat has topic_id = None, so the resolver
    // skips session-origin and falls through to the auto-lookup (None in test).
    let state = empty_state();
    let session_id = Uuid::from_u128(0xBEEF);
    state.register_session_chat(session_id, 12345, None).await;

    let input = json!({});
    let result = resolve_thread_id(&input, 12345, session_id, &state).await;
    assert_eq!(result, None);
}
