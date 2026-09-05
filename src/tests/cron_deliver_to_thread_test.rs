//! The `deliver_to` Telegram grammar carries an optional forum thread.
//!
//! Cron jobs delivering to a forum (topics-enabled) group always landed in
//! the chat's default topic: the delivery leg hardcoded `thread = None` and
//! `telegram:<chat_id>` was the whole grammar (fork #104). The fix is an
//! opt-in `telegram:<chat_id>:<thread_id>` form — never a silent retarget.
//! These tests pin the parse contract every consumer (`deliver_result`,
//! the send-scope permit, the dedup keyboard) goes through.

use crate::cron::scheduler::parse_telegram_target;

/// The plain form every existing job uses must parse exactly as before:
/// chat only, no thread — the default-topic behavior is unchanged.
#[test]
fn plain_chat_target_has_no_thread() {
    assert_eq!(
        parse_telegram_target("-1003936827469"),
        Some((-1003936827469, None))
    );
}

/// The new opt-in form: chat id + forum topic id.
#[test]
fn threaded_target_carries_both_ids() {
    assert_eq!(
        parse_telegram_target("-1003936827469:30220"),
        Some((-1003936827469, Some(30220)))
    );
}

/// Real-world supervisor-group ids are negative; thread ids are positive
/// message ids. Whitespace around components (comma-split leftovers) is
/// tolerated, matching the old `trim` behavior on the chat id.
#[test]
fn whitespace_around_components_is_tolerated() {
    assert_eq!(
        parse_telegram_target(" -1003936827469 : 40695 "),
        Some((-1003936827469, Some(40695)))
    );
}

/// A non-numeric chat id is a malformed target, not a zero chat.
#[test]
fn non_numeric_chat_id_is_rejected() {
    assert_eq!(parse_telegram_target("not-a-chat"), None);
}

/// A non-numeric thread id is a malformed target too — the caller fails
/// loudly; the parser must not hand a partial answer downstream.
#[test]
fn non_numeric_thread_id_is_rejected() {
    assert_eq!(parse_telegram_target("-1003936827469:topic"), None);
}

/// A third `:` segment means the target is malformed, not deeply nested.
#[test]
fn extra_segment_is_rejected() {
    assert_eq!(parse_telegram_target("-1003936827469:30220:extra"), None);
}

/// An empty thread component is malformed.
#[test]
fn empty_thread_component_is_rejected() {
    assert_eq!(parse_telegram_target("-1003936827469:"), None);
}
