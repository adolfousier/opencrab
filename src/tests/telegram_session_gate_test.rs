//! Session resolution is single-flight per `(chat, topic)` (#1201).
//!
//! Resolving a chat to its session is lookup-then-create, and the two steps
//! were not atomic. Two messages into one brand-new forum topic ~88 ms apart
//! both missed the lookup and both created a session: two acks, two full
//! provider calls, and the orphan's context lost once later messages bound to
//! the survivor.
//!
//! Both creations happened on one thread, so this is not a cross-thread data
//! race on a shared map — it is two async tasks interleaving at an await
//! between the lookup and the insert. A lock around the map alone would not
//! close it; the lock has to span both steps, which is what these tests pin.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::channels::telegram::session_gate;

/// A resolve-or-create with an await between the two halves, which is the
/// shape that lost the race.
async fn resolve_or_create(store: &Arc<tokio::sync::Mutex<Option<u64>>>, created: &AtomicUsize) {
    let existing = *store.lock().await;
    // The await that let the second task in.
    tokio::task::yield_now().await;
    if existing.is_none() {
        created.fetch_add(1, Ordering::SeqCst);
        *store.lock().await = Some(1);
    }
}

#[tokio::test]
async fn test_two_near_simultaneous_messages_resolve_one_session() {
    let store = Arc::new(tokio::sync::Mutex::new(None));
    let created = Arc::new(AtomicUsize::new(0));
    const CHAT: i64 = -1003936827469;
    const TOPIC: Option<i32> = Some(30045);

    let tasks: Vec<_> = (0..2)
        .map(|_| {
            let store = Arc::clone(&store);
            let created = Arc::clone(&created);
            tokio::spawn(async move {
                let _gate = session_gate::hold(CHAT, TOPIC).await;
                resolve_or_create(&store, &created).await;
            })
        })
        .collect();
    for t in tasks {
        t.await.expect("resolve task panicked");
    }

    assert_eq!(
        created.load(Ordering::SeqCst),
        1,
        "#1201: the second message must find the first message's session, \
         not create a parallel one"
    );
}

#[tokio::test]
async fn test_without_the_gate_the_same_shape_double_creates() {
    // The bug itself, so the test above is known to be testing something.
    let store = Arc::new(tokio::sync::Mutex::new(None));
    let created = Arc::new(AtomicUsize::new(0));

    let tasks: Vec<_> = (0..2)
        .map(|_| {
            let store = Arc::clone(&store);
            let created = Arc::clone(&created);
            tokio::spawn(async move { resolve_or_create(&store, &created).await })
        })
        .collect();
    for t in tasks {
        t.await.expect("resolve task panicked");
    }

    assert_eq!(
        created.load(Ordering::SeqCst),
        2,
        "#1201: ungated, both tasks miss the lookup and both create"
    );
}

#[tokio::test]
async fn test_different_topics_in_one_chat_do_not_block_each_other() {
    // A forum supergroup is one chat with many topics, and each topic has its
    // own session. Keying the gate on the chat alone would serialize every
    // topic in a busy forum.
    const CHAT: i64 = -1003936827469;
    let held = session_gate::hold(CHAT, Some(101)).await;

    let other = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        session_gate::hold(CHAT, Some(202)),
    )
    .await;
    assert!(
        other.is_ok(),
        "#1201: a second topic must not wait on the first"
    );
    drop(held);
}

#[tokio::test]
async fn test_the_same_key_serializes() {
    const CHAT: i64 = -1000000000001;
    let held = session_gate::hold(CHAT, Some(7)).await;

    // The point of the gate: a second holder of the SAME key waits.
    let blocked = tokio::time::timeout(
        std::time::Duration::from_millis(200),
        session_gate::hold(CHAT, Some(7)),
    )
    .await;
    assert!(
        blocked.is_err(),
        "#1201: the same (chat, topic) must be single-flight"
    );

    drop(held);
    // ...and is handed over once released, rather than deadlocking.
    let after = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        session_gate::hold(CHAT, Some(7)),
    )
    .await;
    assert!(
        after.is_ok(),
        "#1201: the gate must be released, not leaked"
    );
    assert!(session_gate::tracked() > 0);
}

#[tokio::test]
async fn test_general_topic_and_non_forum_share_the_none_key() {
    // `None` is how the rest of the Telegram state already keys the General
    // topic and non-forum groups, so the gate must agree with it.
    const CHAT: i64 = -1000000000002;
    let held = session_gate::hold(CHAT, None).await;
    let same = tokio::time::timeout(
        std::time::Duration::from_millis(200),
        session_gate::hold(CHAT, None),
    )
    .await;
    assert!(same.is_err(), "#1201: None must be a key like any other");
    drop(held);
}
