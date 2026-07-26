//! Card writes are serialised per session (#822).
//!
//! `refresh_plan_card` reads whether a card is tracked, decides to edit or
//! post, then records the id. With nothing held across that, two concurrent
//! refreshes both saw no card, both posted, and the second id overwrote the
//! first. The loser stayed visible in the chat but untracked, so it could
//! never be edited or deleted again: two identical cards, one of them
//! permanent.
//!
//! Concurrency is routine here, not exotic: the streaming path refreshes
//! repeatedly while the settle and resume paths also fire.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::channels::telegram::TelegramState;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use uuid::Uuid;

#[tokio::test]
async fn one_session_serialises_its_card_writes() {
    let state = Arc::new(TelegramState::new());
    let session = Uuid::new_v4();
    // Counts how many tasks are inside the critical section at once. The old
    // bug is exactly two being in there together.
    let inside = Arc::new(AtomicUsize::new(0));
    let max_seen = Arc::new(AtomicUsize::new(0));

    let mut handles = Vec::new();
    for _ in 0..8 {
        let (state, inside, max_seen) = (state.clone(), inside.clone(), max_seen.clone());
        handles.push(tokio::spawn(async move {
            let lock = state.plan_card_lock(session).await;
            let _guard = lock.lock().await;
            let now = inside.fetch_add(1, Ordering::SeqCst) + 1;
            max_seen.fetch_max(now, Ordering::SeqCst);
            // Yield while holding, so an unserialised implementation would
            // reliably overlap here rather than by luck.
            tokio::task::yield_now().await;
            inside.fetch_sub(1, Ordering::SeqCst);
        }));
    }
    for h in handles {
        h.await.unwrap();
    }

    assert_eq!(
        max_seen.load(Ordering::SeqCst),
        1,
        "two refreshes for one session were in the critical section together, \
         which is what posts a duplicate card"
    );
}

#[tokio::test]
async fn different_sessions_do_not_block_each_other() {
    // Per session, not global: a busy chat must not stall an unrelated one.
    let state = Arc::new(TelegramState::new());
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());

    let lock_a = state.plan_card_lock(a).await;
    let held = lock_a.lock().await;

    // B must be acquirable while A is held.
    let lock_b = state.plan_card_lock(b).await;
    assert!(
        lock_b.try_lock().is_ok(),
        "an unrelated session must not wait on this one"
    );
    drop(held);
}

#[tokio::test]
async fn the_same_session_returns_the_same_lock() {
    // Handing out a fresh lock per call would serialise nothing.
    let state = Arc::new(TelegramState::new());
    let session = Uuid::new_v4();

    let first = state.plan_card_lock(session).await;
    let second = state.plan_card_lock(session).await;
    assert!(
        Arc::ptr_eq(&first, &second),
        "a per-call lock protects nothing"
    );
}
