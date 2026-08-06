//! What each session last saw on disk, so a whole-file write cannot silently
//! overwrite another agent's change (#954).
//!
//! Several agents share one working directory by design — every channel gets
//! its own service, `spawn_agent` hands children the parent's directory, and
//! `team_create` launches a whole team into it with `max_concurrent` defaulting
//! to 4. `write_file` replaces a file wholesale with content the agent composed
//! from a read in an EARLIER tool call, so anything written in between is lost
//! with nothing reported.
//!
//! This does not serialise the agents. It only notices when the ground moved:
//! the write is refused and the agent re-reads, which it can do perfectly well.
//! What it cannot do is detect that its write destroyed someone else's work.
//!
//! `edit_file` needs none of this — it re-reads inside the call, so it either
//! applies on top of the newer content or fails to match it.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use uuid::Uuid;

/// (session, canonical path) → hash of the content that session last saw.
type Seen = HashMap<(Uuid, String), u64>;

fn seen() -> &'static Mutex<Seen> {
    static SEEN: OnceLock<Mutex<Seen>> = OnceLock::new();
    SEEN.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Cheap content fingerprint. Collisions only cost a missed warning, never a
/// wrong refusal of an unchanged file, so a full digest buys nothing here.
pub fn hash_content(content: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    content.hash(&mut h);
    h.finish()
}

/// Key on the resolved path so `./x`, `x` and an absolute path agree.
fn key(session_id: Uuid, path: &Path) -> (Uuid, String) {
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    (session_id, resolved.to_string_lossy().into_owned())
}

/// Record what this session now knows the file to contain. Called after a read
/// and after a successful write, so the writer's own output is not mistaken for
/// someone else's change on the next write.
pub fn record(session_id: Uuid, path: &Path, content: &str) {
    if let Ok(mut map) = seen().lock() {
        map.insert(key(session_id, path), hash_content(content));
    }
}

/// Would replacing `path` wholesale destroy a change this session has not seen?
///
/// `on_disk` is the file's current content; `None` means it does not exist yet.
///
/// Deliberately narrow. It fires only when this session read the file AND the
/// file has moved since — the lost update, exactly. A session that never read
/// the file is allowed through: `write_file` is documented as create-or-replace
/// and is legitimately used to generate a file outright, so refusing there
/// would break the tool's contract to guard a case nobody reported. An agent
/// rewriting existing source reads it first, which is what arms this.
pub fn is_stale_write(session_id: Uuid, path: &Path, on_disk: Option<&str>) -> bool {
    let Some(current) = on_disk else {
        return false; // creating a new file races with nobody
    };
    let Ok(map) = seen().lock() else {
        return false;
    };
    match map.get(&key(session_id, path)) {
        Some(recorded) => *recorded != hash_content(current),
        None => false,
    }
}

/// The message the agent gets back. Says what happened and what to do, because
/// a refusal it cannot act on just becomes a retry loop.
pub fn refusal_message(path: &Path) -> String {
    format!(
        "Refusing to overwrite {}: it changed on disk after you read it. \
         Another agent is working in this directory. Read the file again, \
         re-apply your change to the current content, then write. \
         If you only need to change part of it, prefer edit_file — it \
         re-reads the file itself and cannot clobber a concurrent edit.",
        path.display()
    )
}

/// Drop everything remembered for a session. Sub-agent sessions are created per
/// spawn and never revisited, so without this the map grows for the life of the
/// process.
pub fn forget_session(session_id: Uuid) {
    if let Ok(mut map) = seen().lock() {
        map.retain(|(sid, _), _| *sid != session_id);
    }
}
