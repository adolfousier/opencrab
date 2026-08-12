//! Keep the index current for brain files that changed outside the write tool
//! (#1018).
//!
//! The index used to be a boot-time snapshot: a full `reindex` ten seconds
//! after startup, plus one incremental call for the current daily note. Nothing
//! else refreshed it, so a brain file written mid-session stayed stale until the
//! next restart — measured at fifteen hours on a live install. A rule appended
//! now was invisible to `memory_search`, which is exactly where the duplicate
//! check in #1017 needs it to be visible.
//!
//! Writes are the real fix and are handled at the write site. This is the net
//! under that, for writes the tool never saw: hand edits, another profile, an
//! editor left open.
//!
//! ## Why mtime and not content hash
//!
//! Hashing means reading every file. `documents.modified_at` is already stored,
//! so comparing it against filesystem mtime is a stat per file and no read. On
//! an unchanged workspace the whole check is nine stats and no work; only files
//! whose mtime actually moved are read and reindexed, where the existing
//! hash-skip inside `index_file_sync` still suppresses embedding if the content
//! turns out to be identical.
//!
//! ## Why brain files only
//!
//! They are the ones written mid-session and read by a duplicate check, and
//! they are a bounded set. Daily notes are append-only history, already covered
//! by the compaction hook, and there are 158 of them on the same install —
//! walking those on every search is a tax paid constantly to catch a change in
//! one of nine.
//!
//! ## Single flight
//!
//! Refresh runs on a user-triggered path, so two searches can land together. The
//! startup reindex is deliberately delayed and spawned because "llama-cpp GGML
//! can segfault under contention", and embedding is exactly what a changed file
//! triggers. A second caller therefore skips rather than waits: the first will
//! finish, and a search served against a one-call-old index is a far smaller
//! problem than the crash.

use std::collections::HashMap;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

/// mtime of each brain file the last time THIS process indexed it.
///
/// `documents.modified_at` is not bumped when a document's content is updated,
/// so comparing against it was permanently true: the check reported the same
/// files stale on every single search and reindexed them forever (#1021,
/// observed as repeated "reindexed 2 stale brain file(s)" in a live log).
///
/// Tracking what this process actually indexed makes the comparison honest. A
/// restart re-checks everything once, which is correct — the startup reindex
/// runs then anyway.
static LAST_INDEXED: StdMutex<Option<HashMap<String, SystemTime>>> = StdMutex::new(None);

/// Set while a refresh is running, so a concurrent search skips instead of
/// entering embedding alongside it.
static REFRESHING: AtomicBool = AtomicBool::new(false);

/// Reindex brain files whose mtime is newer than what the index recorded.
///
/// Returns the number of files reindexed. Never fails the caller: a search must
/// still run against a stale index rather than error, so problems are logged
/// and the count reflects only what actually succeeded.
pub async fn refresh_stale_brain_files() -> usize {
    if REFRESHING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        tracing::debug!("Memory freshness: refresh already in flight, skipping");
        return 0;
    }

    let refreshed = refresh_inner().await;
    REFRESHING.store(false, Ordering::Release);
    refreshed
}

async fn refresh_inner() -> usize {
    let home = crate::config::opencrabs_home();
    let mut refreshed = 0usize;

    for &name in super::BRAIN_FILES {
        let path = home.join(name);
        let disk_mtime = match std::fs::metadata(&path).and_then(|m| m.modified()) {
            Ok(t) => t,
            // Not every brain file exists in every workspace.
            Err(_) => continue,
        };

        let seen = {
            let guard = LAST_INDEXED.lock().unwrap_or_else(|e| e.into_inner());
            guard.as_ref().and_then(|m| m.get(name).copied())
        };
        let changed = match seen {
            Some(t) => disk_mtime > t,
            // Not yet seen by this process: check it once.
            None => true,
        };

        if changed && index_one(&path, name).await {
            refreshed += 1;
            let mut guard = LAST_INDEXED.lock().unwrap_or_else(|e| e.into_inner());
            guard
                .get_or_insert_with(HashMap::new)
                .insert(name.to_string(), disk_mtime);
        }
    }

    if refreshed > 0 {
        tracing::info!("Memory freshness: reindexed {refreshed} stale brain file(s)");
    }
    refreshed
}

/// Index one file, logging rather than propagating.
async fn index_one(path: &std::path::Path, name: &str) -> bool {
    let store = match super::get_store() {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("Memory freshness: store unavailable for {name}: {e}");
            return false;
        }
    };
    match super::index_file_fts_only(store, path).await {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!("Memory freshness: failed to reindex {name}: {e}");
            false
        }
    }
}
