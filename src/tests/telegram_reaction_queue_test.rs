//! Tests for the mid-turn reaction queue and active-turn tracking (#302 Stage 2).

use crate::channels::telegram::TelegramState;
use std::sync::Arc;
use uuid::Uuid;

#[test]
fn reactions_drain_fifo_per_session() {
    let state = Arc::new(TelegramState::new());
    let s1 = Uuid::new_v4();
    let s2 = Uuid::new_v4();
    state.enqueue_reaction(s1, "first".to_string());
    state.enqueue_reaction(s1, "second".to_string());
    state.enqueue_reaction(s2, "other".to_string());

    // FIFO drain, and s2's queue is never touched by draining s1.
    assert_eq!(state.drain_reaction(s1).as_deref(), Some("first"));
    assert_eq!(state.drain_reaction(s1).as_deref(), Some("second"));
    assert_eq!(state.drain_reaction(s1), None);
    assert_eq!(state.drain_reaction(s2).as_deref(), Some("other"));
    assert_eq!(state.drain_reaction(s2), None);
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
    state.enqueue_reaction(sid, "queued".to_string());

    let cb = state.reaction_queue_callback();
    assert_eq!(cb(sid).await.as_deref(), Some("queued"));
    // Drained — nothing left.
    assert_eq!(cb(sid).await, None);
}
