//! Shared per-key single-flight gate (#1201, generalized #1228).
//!
//! Session resolution is lookup-then-create, and the two steps are not
//! atomic. Two messages landing in a brand-new chat/topic ~90 ms apart both
//! miss the lookup and both create a session, so two full turns run in
//! parallel against one key and the orphaned session's context is lost.
//!
//! The "session busy, queue into the in-flight turn" guard cannot help with
//! this, because neither turn is streaming yet when the other resolves — the
//! race is entirely inside the window before any session exists.
//!
//! Both creations happen on one thread, so this is not a cross-thread data
//! race on a shared map. It is two independent async tasks interleaving at an
//! await point between the lookup and the insert, which a `Mutex` around the
//! map alone would not close: the lock has to span BOTH steps. That is what
//! this module provides — a per-key async gate whose guard lives for the whole
//! resolve-or-create.
//!
//! Originally written for the Telegram forum-topic case keyed on `(chat_id,
//! topic_id)` (#1201). Generalizing the key to `String` lets every channel
//! resolver reuse the same gate instead of re-implementing it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

fn gates() -> &'static Mutex<HashMap<String, Arc<AsyncMutex<()>>>> {
    static GATES: OnceLock<Mutex<HashMap<String, Arc<AsyncMutex<()>>>>> = OnceLock::new();
    GATES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Hold the resolution gate for `key` until the guard drops.
///
/// The second caller waits for the first to finish resolving, and then finds
/// the session the first created rather than creating its own.
///
/// `key` must identify the entity whose resolution must be single-flight
/// (chat id, chat+topic pair, channel id — whatever the caller resolves by).
/// Distinct keys never block each other.
///
/// A poisoned registry is recovered from rather than propagated: refusing to
/// resolve a session is a worse outcome than the duplicate this prevents.
pub async fn hold(key: impl Into<String>) -> OwnedMutexGuard<()> {
    let key = key.into();
    let gate = {
        let mut map = gates().lock().unwrap_or_else(|e| e.into_inner());
        Arc::clone(map.entry(key).or_default())
    };
    gate.lock_owned().await
}

/// How many keys are being tracked. Exposed for tests.
pub fn tracked() -> usize {
    gates().lock().unwrap_or_else(|e| e.into_inner()).len()
}