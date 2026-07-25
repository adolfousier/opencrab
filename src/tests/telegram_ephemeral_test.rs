//! Ephemeral group replies: Bot API 10.2 `receiver_user_id` (#756).
//!
//! Covers the two pure pieces: who a reply is scoped to, and the request
//! shape sent to `sendMessage`. The transport itself needs a live bot.

use crate::channels::telegram::ephemeral::{build_body, receiver_for};
use teloxide::types::{MessageId, ThreadId};

#[test]
fn dm_never_scopes_a_reply() {
    // A DM has nobody to hide the reply from, and asking for an untested
    // parameter there would risk the one path that already works.
    assert_eq!(receiver_for(true, 12345), None);
}

#[test]
fn group_scopes_the_reply_to_the_invoker() {
    assert_eq!(receiver_for(false, 12345), Some(12345));
}

#[test]
fn body_carries_receiver_user_id() {
    let body = build_body(-100200, None, 12345, "hello", false);
    assert_eq!(body["chat_id"], -100200);
    assert_eq!(body["text"], "hello");
    assert_eq!(body["receiver_user_id"], 12345);
}

#[test]
fn plain_body_has_no_parse_mode() {
    // Acks like "✅ New session started." are literal text, and an HTML parse
    // mode would swallow any `<` or `&` they happen to contain.
    let body = build_body(-100200, None, 12345, "a < b & c", false);
    assert!(body.get("parse_mode").is_none());
}

#[test]
fn html_body_sets_parse_mode() {
    let body = build_body(-100200, None, 12345, "<b>hi</b>", true);
    assert_eq!(body["parse_mode"], "HTML");
}

#[test]
fn thread_id_targets_the_forum_topic() {
    let body = build_body(-100200, Some(ThreadId(MessageId(77))), 12345, "hi", true);
    assert_eq!(body["message_thread_id"], 77);
}

#[test]
fn no_thread_id_omits_the_field() {
    // Sending `message_thread_id: null` to a non-forum chat is an API error,
    // so the field has to be absent rather than explicitly empty.
    let body = build_body(-100200, None, 12345, "hi", true);
    assert!(body.get("message_thread_id").is_none());
}
