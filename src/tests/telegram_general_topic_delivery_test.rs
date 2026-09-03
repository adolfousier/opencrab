//! A forum's General topic is addressed by the ABSENCE of a thread (#1319).
//!
//! `GENERAL_TOPIC_ID = 1` is a session-scoping key, invented by #1220 so
//! General would not collide with a DM on the same chat. It is not an address:
//! General messages carry no `message_thread_id`, so putting `1` on the wire
//! asks for a thread that does not exist and every message-CREATING call is
//! refused with `Bad Request: message thread not found`.
//!
//! Edits masked it for a long time — they address a `message_id` and never
//! consult a thread — so a General-bound session kept updating its status
//! cards in place and failed only when it had something new to say. Measured
//! over one 16-minute window: 69 edits succeeded, 308 creates failed, every
//! failure carrying `thread=Some(1)`.

use serde_json::json;

use crate::channels::telegram::session_resolve::{
    GENERAL_TOPIC_ID, delivery_thread_id, normalize_topic, session_topic_for_event,
};

/// A real forum topic id, comfortably clear of the General sentinel.
const REAL_TOPIC: i32 = 30045;

// ── The boundary ──────────────────────────────────────────────────────────

#[test]
fn test_general_resolves_to_no_thread_on_the_wire() {
    assert_eq!(
        delivery_thread_id(Some(GENERAL_TOPIC_ID)),
        None,
        "#1319: sending thread 1 is what Telegram refuses"
    );
}

#[test]
fn test_a_real_topic_is_addressed_normally() {
    // The #215 / #1200 routing must not regress: a real topic still gets its
    // thread, or every forum reply lands in General.
    let addressed = delivery_thread_id(Some(REAL_TOPIC)).expect("a real topic keeps its thread");
    assert_eq!(addressed.0.0, REAL_TOPIC);
}

#[test]
fn test_no_topic_stays_no_thread() {
    assert_eq!(delivery_thread_id(None), None);
}

// ── The scoping key must survive ──────────────────────────────────────────

#[test]
fn test_general_keeps_its_scoping_key_distinct_from_a_dm() {
    // #1220's whole purpose. The obvious "fix" for this bug is to collapse
    // General back to None everywhere, which silently undoes that and makes a
    // forum's General share one session with a DM on the same chat.
    let general = session_topic_for_event(false, None, true);
    let dm = session_topic_for_event(false, None, false);

    assert_eq!(general, Some(GENERAL_TOPIC_ID));
    assert_eq!(dm, None);
    assert_ne!(
        general, dm,
        "#1220: General and a DM must not share a session key"
    );
}

#[test]
fn test_the_key_and_the_address_disagree_on_purpose() {
    // The heart of #1319: one value served both roles, so the key leaked onto
    // the wire. They are allowed to differ, and for General they MUST.
    let key = normalize_topic(None, true);
    assert_eq!(key, Some(GENERAL_TOPIC_ID), "scoping key is the sentinel");
    assert_eq!(
        delivery_thread_id(key),
        None,
        "delivery address is the absence of a thread"
    );
}

#[test]
fn test_a_real_topic_agrees_on_both() {
    let key = normalize_topic(Some(REAL_TOPIC), true);
    assert_eq!(key, Some(REAL_TOPIC));
    assert_eq!(
        delivery_thread_id(key).map(|t| t.0.0),
        Some(REAL_TOPIC),
        "only General is special; a real topic is its own address"
    );
}

#[test]
fn test_a_non_forum_group_gets_no_synthetic_key() {
    assert_eq!(session_topic_for_event(false, None, false), None);
    assert_eq!(delivery_thread_id(None), None);
}

// ── The tool's precedence ─────────────────────────────────────────────────

/// `resolve_thread_id` needs live state, so these cover the input-parsing
/// half: the three caller intents must be distinguishable from the JSON
/// alone, which is what was impossible before.
#[test]
fn test_explicit_null_is_distinguishable_from_an_absent_field() {
    let explicit_general = json!({ "message": "hi", "thread_id": null });
    let unspecified = json!({ "message": "hi" });

    assert!(
        matches!(
            explicit_general.get("thread_id"),
            Some(serde_json::Value::Null)
        ),
        "#1319: 'post to General' must be expressible"
    );
    assert!(
        unspecified.get("thread_id").is_none(),
        "'wherever this session lives' is the ABSENT case, and must stay distinct"
    );
}

#[test]
fn test_naming_thread_one_means_general_not_thread_one() {
    // Reproduces the live differential test on the issue: the agent named
    // thread 1 for #general and the send was refused, because 1 went on the
    // wire verbatim.
    assert_eq!(
        delivery_thread_id(Some(1)),
        None,
        "#1319: an agent naming thread 1 means General"
    );
}

#[test]
fn test_round_trip_from_a_general_message_to_its_delivery_address() {
    // End to end over the two functions a message actually crosses: an
    // incoming General message (no thread, not flagged a topic message, in a
    // chat known to be a forum) must come back out as "no thread".
    let key = session_topic_for_event(/* is_topic_message */ false, None, true);
    assert_eq!(key, Some(GENERAL_TOPIC_ID));
    assert_eq!(
        delivery_thread_id(key),
        None,
        "#1319: this is the round trip that dropped 308 messages"
    );
}

#[test]
fn test_round_trip_for_a_real_topic_is_unchanged() {
    let key = session_topic_for_event(true, Some(REAL_TOPIC), true);
    assert_eq!(key, Some(REAL_TOPIC));
    assert_eq!(delivery_thread_id(key).map(|t| t.0.0), Some(REAL_TOPIC));
}
