//! Single-flight session resolution per `(chat, topic)` (#1201).
//!
//! Resolving a chat to its session is lookup-then-create, and the two steps
//! were not atomic. Two messages landing in the same brand-new forum topic
//! ~88 ms apart both missed the lookup and both created a session, so two
//! full turns ran in parallel against one topic: two acks, two provider
//! calls, and the orphaned session's context lost when later messages bound
//! to the survivor.
//!
//! The existing "session busy, queue into the in-flight turn" guard cannot
//! help, because neither turn was streaming yet when the other resolved — the
//! race is entirely inside the window before either session exists.
//!
//! Both creations were on one thread, so this is not a cross-thread data
//! race on a shared map. It is two independent async tasks interleaving at an
//! await point between the lookup and the insert, which a `Mutex` around the
//! map alone would not close: the lock has to span BOTH steps.
//!
//! Scope matters. The guard covers resolution and the chat→session
//! registration that follows it, and is released before the turn runs.
//! Holding it for the whole turn would serialize a topic's messages, which
//! would defeat the mid-turn queueing that #302 exists to provide.
//!
//! The gate itself lives in [`crate::channels::single_flight`], which owns
//! the `OnceLock<Mutex<HashMap<_, Arc<AsyncMutex>>>>` registry keyed by
//! `String`. This module is a thin typed wrapper over it so the Telegram
//! resolver keeps a `(chat_id, topic_id)` API. There is exactly one
//! implementation; every channel maps its own key type onto the shared one.

use tokio::sync::OwnedMutexGuard;

use crate::channels::single_flight;

/// Render a `(chat_id, topic_id)` pair as the shared gate's `String` key.
///
/// `None` (the General topic and non-forum groups) must map onto a distinct
/// key so it is serialized like any other, matching how the rest of the
/// Telegram state keys them.
fn key(chat_id: i64, topic_id: Option<i32>) -> String {
    match topic_id {
        Some(topic) => format!("{chat_id}:{topic}"),
        None => format!("{chat_id}:no-topic"),
    }
}

/// Hold the resolution gate for `(chat_id, topic_id)` until the guard drops.
///
/// The second caller waits for the first to finish resolving, and then finds
/// the session the first created rather than creating its own.
///
/// A poisoned registry is recovered from rather than propagated: refusing to
/// resolve a session is a worse outcome than the duplicate this prevents.
pub(crate) async fn hold(chat_id: i64, topic_id: Option<i32>) -> OwnedMutexGuard<()> {
    single_flight::hold(key(chat_id, topic_id)).await
}

/// How many gates are being tracked. Exposed for tests.
#[cfg(test)]
pub(crate) fn tracked() -> usize {
    single_flight::tracked()
}
