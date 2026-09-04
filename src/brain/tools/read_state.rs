//! Session-scoped fully-read path tracking (#1168).
//!
//! `read_file` records paths it has shown in full during a session;
//! `write_file` consults the record before overwriting an existing file so a
//! windowed read cannot silently destroy lines the model never saw.
//!
//! State is keyed by `(session_id, canonical path)` and held process-wide:
//! tool instances are constructed independently, so per-instance maps would
//! never meet. Test isolation falls out of the session key — each test
//! context carries its own fresh UUID.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use uuid::Uuid;

fn registry() -> &'static std::sync::Mutex<HashSet<(Uuid, PathBuf)>> {
    static REGISTRY: OnceLock<std::sync::Mutex<HashSet<(Uuid, PathBuf)>>> = OnceLock::new();
    REGISTRY.get_or_init(|| std::sync::Mutex::new(HashSet::new()))
}

/// Best-effort canonicalization so `./a.txt`, `a.txt`, and an absolute path
/// to the same file collapse onto one key.
fn key_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Record that `session_id` was shown the full contents of `path`.
pub fn mark_fully_read(session_id: Uuid, path: &Path) {
    registry()
        .lock()
        .expect("read_state registry poisoned")
        .insert((session_id, key_path(path)));
}

/// Whether `session_id` has been shown the full contents of `path` this run.
pub fn was_fully_read(session_id: Uuid, path: &Path) -> bool {
    registry()
        .lock()
        .expect("read_state registry poisoned")
        .contains(&(session_id, key_path(path)))
}
