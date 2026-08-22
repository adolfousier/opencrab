//! Newest-incoming-message tracking used by the flow-block re-stick (#451).
//!
//! The streaming edit loop relocates its open processing-log block to the
//! bottom when newer chatter has buried it. That decision reads the highest
//! incoming message id the handler has recorded for the chat. These tests pin
//! the per-chat max bookkeeping the re-stick relies on.

use std::time::Duration;

use crate::channels::telegram::TelegramState;

#[test]
fn newest_msg_id_starts_empty() {
    let state = TelegramState::new();
    assert_eq!(state.newest_incoming_msg_id(100), None);
}

#[test]
fn newest_msg_id_keeps_the_per_chat_maximum() {
    let state = TelegramState::new();
    state.note_incoming_msg(100, 10);
    state.note_incoming_msg(100, 25);
    state.note_incoming_msg(100, 25);
    assert_eq!(state.newest_incoming_msg_id(100), Some(25));

    // An out-of-order (lower) id must not lower the recorded max: Telegram ids
    // are monotonic per chat, so a smaller id is stale, never the newest.
    state.note_incoming_msg(100, 12);
    assert_eq!(state.newest_incoming_msg_id(100), Some(25));
}

#[test]
fn newest_msg_id_is_isolated_per_chat() {
    let state = TelegramState::new();
    state.note_incoming_msg(100, 50);
    state.note_incoming_msg(200, 7);
    assert_eq!(state.newest_incoming_msg_id(100), Some(50));
    assert_eq!(state.newest_incoming_msg_id(200), Some(7));
    assert_eq!(state.newest_incoming_msg_id(300), None);
}

#[test]
fn block_is_buried_only_when_a_newer_message_landed() {
    // Mirrors the restick gate: buried iff newest incoming id > block id.
    let state = TelegramState::new();
    let block_mid = 40;

    // Nothing newer than the block: not buried.
    state.note_incoming_msg(100, 40);
    assert!(state.newest_incoming_msg_id(100).unwrap() <= block_mid);

    // A message lands after the block: buried, re-stick fires.
    state.note_incoming_msg(100, 41);
    assert!(state.newest_incoming_msg_id(100).unwrap() > block_mid);
}

// ── #1150: bot bubbles are burial evidence; sticky actions share one budget ─

#[test]
fn bot_bubbles_count_as_burial_evidence() {
    // An intermediate report (#582) or follow-up question lands as its own
    // bubble BELOW the flow block. Before #1150 nothing recorded it, so the
    // block stayed buried under its own output for the rest of the turn.
    let state = TelegramState::new();
    state.note_incoming_msg(100, 40); // the flow block's id
    assert!(state.newest_incoming_msg_id(100).unwrap() <= 40);

    state.note_bot_bubble(100, 55);
    assert_eq!(state.newest_incoming_msg_id(100), Some(55));
}

#[test]
fn budget_admits_one_action_then_gates_within_the_interval() {
    let state = TelegramState::new();
    assert!(
        state.claim_sticky_action(100, Duration::from_secs(15)),
        "first action is always admitted"
    );
    assert!(
        !state.claim_sticky_action(100, Duration::from_secs(15)),
        "a second delete+create inside the window must be denied (#814)"
    );

    // A different chat has its own budget — one chat's churn never gates
    // another's.
    assert!(state.claim_sticky_action(200, Duration::from_secs(15)));
}

#[test]
fn zero_interval_budget_is_always_admissible() {
    // The escape hatch callers use when a move MUST happen now (e.g. tests or
    // an explicit user-triggered reorder).
    let state = TelegramState::new();
    assert!(state.claim_sticky_action(100, Duration::ZERO));
    assert!(state.claim_sticky_action(100, Duration::ZERO));
}
