//! The shared/group session gate for external memory results (#1051, ADR-003).
//!
//! The external session gate must know whether the current session is a
//! shared/group chat (several people can read the reply) or the owner's own
//! session. Channel handlers know that when they resolve a session — they see
//! the chat type — so they mark it here; `memory_search` checks it before
//! returning external content. Process-local by design: on restart the set is
//! empty and each channel re-marks its group sessions on first use, which is
//! harmless because the gate only ever denies until then.

use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};

use uuid::Uuid;

/// Session IDs of shared/group channel sessions.
static SHARED_SESSIONS: LazyLock<Mutex<HashSet<Uuid>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

/// Mark a session as a shared/group channel session (#1051). Called by the
/// channel handlers when they resolve a session for a group chat.
pub fn mark_session_shared(session_id: Uuid) {
    if let Ok(mut g) = SHARED_SESSIONS.lock() {
        g.insert(session_id);
    }
}

/// Whether a session is a shared/group channel session (#1051). Consulted by
/// the `memory_search` external gate.
pub fn is_session_shared(session_id: Uuid) -> bool {
    SHARED_SESSIONS
        .lock()
        .map(|g| g.contains(&session_id))
        .unwrap_or(false)
}
