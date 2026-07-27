//! One streaming turn per session when background tasks finish together (#845).
//!
//! Each detached command that completed spawned its own full streaming resume
//! turn. With several finishing at once, every turn opened and edited its own
//! flow block in the same chat: five resumes on one session inside 850ms, and
//! duplicated, overlapping output.
//!
//! The background producer was the only path skipping the active-turn gate that
//! user messages and reactions have used since #501. These pin the two halves
//! of the gate it now goes through: the claim is exclusive, and a loser hands
//! its result to the turn already running rather than forking a second one.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::brain::agent::QueuedUserMessage;
use crate::channels::telegram::TelegramState;
use std::sync::Arc;
use uuid::Uuid;

#[test]
fn only_the_first_of_several_completions_claims_the_turn() {
    let state = Arc::new(TelegramState::new());
    let sid = Uuid::new_v4();

    let first = state.try_begin_turn(sid);
    assert!(first.is_some(), "the first completion must claim the turn");

    // The other four in the observed burst.
    for n in 0..4 {
        assert!(
            state.try_begin_turn(sid).is_none(),
            "completion {n} claimed a second concurrent turn"
        );
    }
}

#[test]
fn a_losing_completion_hands_its_result_to_the_running_turn() {
    // Losing the claim must not drop the background result: it goes on the
    // per-session queue the tool loop drains between rounds, so it lands in
    // the block that is already open.
    let state = Arc::new(TelegramState::new());
    let sid = Uuid::new_v4();

    let _running = state.try_begin_turn(sid).expect("first claims");
    assert!(state.try_begin_turn(sid).is_none());
    state.enqueue_reaction(sid, QueuedUserMessage::plain("build finished".to_string()));

    assert_eq!(
        state.drain_reaction(sid).map(|m| m.context_text).as_deref(),
        Some("build finished"),
        "the queued result must be recoverable by the in-flight turn"
    );
}

#[test]
fn every_queued_result_survives_in_order() {
    // Four completions losing the claim must all be delivered, oldest first.
    let state = Arc::new(TelegramState::new());
    let sid = Uuid::new_v4();
    let _running = state.try_begin_turn(sid).expect("first claims");

    for label in ["one", "two", "three", "four"] {
        state.enqueue_reaction(sid, QueuedUserMessage::plain(label.to_string()));
    }
    let mut seen = Vec::new();
    while let Some(m) = state.drain_reaction(sid) {
        seen.push(m.context_text);
    }
    assert_eq!(seen, vec!["one", "two", "three", "four"]);
}

#[test]
fn the_next_completion_can_claim_once_the_turn_ends() {
    // The gate must not wedge the session: a background result arriving after
    // the turn finishes has to open its own block, not queue forever.
    let state = Arc::new(TelegramState::new());
    let sid = Uuid::new_v4();
    {
        let _running = state.try_begin_turn(sid).expect("first claims");
        assert!(state.try_begin_turn(sid).is_none());
    }
    assert!(
        state.try_begin_turn(sid).is_some(),
        "the session stayed busy after its turn ended"
    );
}

#[test]
fn sessions_do_not_gate_each_other() {
    // A background task finishing in one chat must never block a resume in
    // another.
    let state = Arc::new(TelegramState::new());
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    let _busy = state.try_begin_turn(a).expect("a claims");
    assert!(
        state.try_begin_turn(b).is_some(),
        "an unrelated session was gated"
    );
}

#[test]
fn the_queue_is_drained_through_the_callback_the_tool_loop_uses() {
    // Enqueuing is only useful if the rail the tool loop pulls from is the same
    // one. If these ever diverge the result would queue and never be delivered.
    let state = Arc::new(TelegramState::new());
    let sid = Uuid::new_v4();
    let _running = state.try_begin_turn(sid).expect("first claims");
    state.enqueue_reaction(sid, QueuedUserMessage::plain("result".to_string()));

    let cb = state.reaction_queue_callback();
    let drained = futures::executor::block_on(cb(sid));
    assert_eq!(
        drained.map(|m| m.context_text).as_deref(),
        Some("result"),
        "the tool loop's callback did not see the queued background result"
    );
}
