//! Tests for the mid-turn reaction queue and active-turn tracking (#302 Stage 2).

use crate::brain::agent::QueuedUserMessage;
use crate::channels::telegram::TelegramState;
use std::sync::Arc;
use uuid::Uuid;

#[test]
fn reactions_drain_fifo_per_session() {
    let state = Arc::new(TelegramState::new());
    let s1 = Uuid::new_v4();
    let s2 = Uuid::new_v4();
    state.enqueue_reaction(s1, QueuedUserMessage::plain("first".to_string()));
    state.enqueue_reaction(s1, QueuedUserMessage::plain("second".to_string()));
    state.enqueue_reaction(s2, QueuedUserMessage::plain("other".to_string()));

    // FIFO drain, and s2's queue is never touched by draining s1.
    let drained = |v: Option<QueuedUserMessage>| v.map(|m| m.context_text);
    assert_eq!(drained(state.drain_reaction(s1)).as_deref(), Some("first"));
    assert_eq!(drained(state.drain_reaction(s1)).as_deref(), Some("second"));
    assert!(state.drain_reaction(s1).is_none());
    assert_eq!(drained(state.drain_reaction(s2)).as_deref(), Some("other"));
    assert!(state.drain_reaction(s2).is_none());
}

#[test]
fn active_turn_guard_marks_and_clears_on_drop() {
    let state = Arc::new(TelegramState::new());
    let sid = Uuid::new_v4();
    assert!(!state.is_turn_active(sid));
    {
        let _guard = state.mark_turn_active(sid);
        assert!(state.is_turn_active(sid));
    }
    // Guard dropped (including on panic/early return in real code) → inactive.
    assert!(!state.is_turn_active(sid));
}

#[test]
fn active_turns_are_independent_per_session() {
    let state = Arc::new(TelegramState::new());
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    let guard_a = state.mark_turn_active(a);
    assert!(state.is_turn_active(a));
    assert!(!state.is_turn_active(b));
    drop(guard_a);
    assert!(!state.is_turn_active(a));
}

#[tokio::test]
async fn queue_callback_drains_the_same_state() {
    // The callback the tool loop invokes between rounds must pull from the very
    // queue handle_reaction enqueues into.
    let state = Arc::new(TelegramState::new());
    let sid = Uuid::new_v4();
    state.enqueue_reaction(sid, QueuedUserMessage::plain("queued".to_string()));

    let cb = state.reaction_queue_callback();
    assert_eq!(
        cb(sid).await.map(|m| m.context_text).as_deref(),
        Some("queued")
    );
    // Drained — nothing left.
    assert!(cb(sid).await.is_none());
}

// ── atomic turn claim (#501) ─────────────────────────────────────────

#[test]
fn try_begin_turn_is_atomic_check_and_set() {
    let state = Arc::new(TelegramState::new());
    let s = Uuid::new_v4();

    // Idle session: first claim succeeds and marks active.
    let guard = state.try_begin_turn(s).expect("first claim succeeds");
    assert!(state.is_turn_active(s));

    // Second claim while the first turn is in flight returns None (the
    // caller enqueues instead of forking a parallel turn) — the race the
    // old check-then-mark window allowed.
    assert!(
        state.try_begin_turn(s).is_none(),
        "concurrent claim must be refused while a turn is active"
    );

    // Dropping the guard clears the flag; the session can be claimed again.
    drop(guard);
    assert!(!state.is_turn_active(s));
    assert!(state.try_begin_turn(s).is_some(), "reclaim after turn ends");
}

#[test]
fn try_begin_turn_is_per_session() {
    let state = Arc::new(TelegramState::new());
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    let _ga = state.try_begin_turn(a).expect("a claims");
    // A being active must not block a different session B.
    assert!(state.try_begin_turn(b).is_some(), "b is independent of a");
    assert!(state.try_begin_turn(a).is_none(), "a still blocked");
}
