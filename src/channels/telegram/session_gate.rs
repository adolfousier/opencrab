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

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

/// One session may exist per chat and topic, so that pair is the key. The
/// General topic and a non-forum group both key on `None`, which is what the
/// rest of the Telegram state already does.
type Key = (i64, Option<i32>);

fn gates() -> &'static Mutex<HashMap<Key, Arc<AsyncMutex<()>>>> {
    static GATES: OnceLock<Mutex<HashMap<Key, Arc<AsyncMutex<()>>>>> = OnceLock::new();
    GATES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Hold the resolution gate for `(chat_id, topic_id)` until the guard drops.
///
/// The second caller waits for the first to finish resolving, and then finds
/// the session the first created rather than creating its own.
///
/// A poisoned registry is recovered from rather than propagated: refusing to
/// resolve a session is a worse outcome than the duplicate this prevents.
pub(crate) async fn hold(chat_id: i64, topic_id: Option<i32>) -> OwnedMutexGuard<()> {
    let gate = {
        let mut map = gates().lock().unwrap_or_else(|e| e.into_inner());
        Arc::clone(map.entry((chat_id, topic_id)).or_default())
    };
    gate.lock_owned().await
}

/// How many gates are being tracked. Exposed for tests.
#[cfg(test)]
pub(crate) fn tracked() -> usize {
    gates().lock().unwrap_or_else(|e| e.into_inner()).len()
}
