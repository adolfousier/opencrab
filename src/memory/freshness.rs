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

use super::db::Store;
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

/// mtime of each EXTERNAL file the last time THIS process indexed it (#1051,
/// ADR-002 tier 1). Keyed by absolute path — external documents are keyed by
/// absolute path, unlike brain files which use basenames. Same honesty rule as
/// LAST_INDEXED: track what this process actually indexed, not the DB timestamp.
static EXTERNAL_LAST_INDEXED: StdMutex<Option<HashMap<String, SystemTime>>> = StdMutex::new(None);

/// Set while an external refresh is running, so a concurrent search skips
/// instead of double-indexing (mirrors REFRESHING).
static EXTERNAL_REFRESHING: AtomicBool = AtomicBool::new(false);

/// Tier-1 external freshness (#1051, ADR-002): reindex the external files
/// behind search hits whose mtime moved. The tier-2 sweep catches additions
/// and deletions via directory mtimes, but an in-place edit does not bump the
/// parent dir's mtime — that is exactly what this check catches, lazily, only
/// for files that actually surfaced as hits. FTS-only: embedding stays off the
/// search path (#1021).
///
/// Returns the number of files reindexed. Never fails the caller: a search
/// must still run against a stale index rather than error.
pub async fn refresh_stale_external(paths: &[String]) -> usize {
    if paths.is_empty() {
        return 0;
    }
    if EXTERNAL_REFRESHING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        tracing::debug!("Memory external freshness: refresh already in flight, skipping");
        return 0;
    }
    let refreshed = refresh_external_inner(paths).await;
    EXTERNAL_REFRESHING.store(false, Ordering::Release);
    refreshed
}

async fn refresh_external_inner(paths: &[String]) -> usize {
    let store = match super::get_store() {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("Memory external freshness: store unavailable: {e}");
            return 0;
        }
    };
    let mut refreshed = 0usize;

    for key in paths {
        let path = std::path::Path::new(key);
        // External documents are keyed by absolute path; anything else is not
        // an external hit.
        if !path.is_absolute() {
            continue;
        }
        let disk_mtime = match std::fs::metadata(path).and_then(|m| m.modified()) {
            Ok(t) => t,
            // Deleted on disk: the tier-2 sweep prunes it, nothing to refresh.
            Err(_) => continue,
        };
        let seen = {
            let guard = EXTERNAL_LAST_INDEXED.lock().unwrap_or_else(|e| e.into_inner());
            guard.as_ref().and_then(|m| m.get(key).copied())
        };
        let changed = match seen {
            Some(t) => disk_mtime > t,
            // Not yet seen by this process: check it once. Hash-skip inside
            // index_file_sync_keyed dedupes if the content is unchanged.
            None => true,
        };
        if !changed {
            continue;
        }
        if index_external_one(store, key, path).await {
            refreshed += 1;
            let mut guard = EXTERNAL_LAST_INDEXED.lock().unwrap_or_else(|e| e.into_inner());
            guard
                .get_or_insert_with(HashMap::new)
                .insert(key.clone(), disk_mtime);
        }
    }

    if refreshed > 0 {
        tracing::info!("Memory external freshness: reindexed {refreshed} stale external file(s)");
    }
    refreshed
}

/// Index one external file by its absolute key. FTS-only (no embedding).
async fn index_external_one(
    store: &'static StdMutex<Store>,
    key: &str,
    path: &std::path::Path,
) -> bool {
    let body = match tokio::fs::read_to_string(path).await {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!("Memory external freshness: unreadable {key}: {e}");
            return false;
        }
    };
    let key = key.to_string();
    let log_key = key.clone();
    let result = tokio::task::spawn_blocking(move || {
        let s = store
            .lock()
            .map_err(|e| format!("Store lock poisoned: {e}"))?;
        super::index::index_file_sync_keyed(&s, super::COLLECTION_EXTERNAL, &key, &body)
    })
    .await;
    match result {
        Ok(Ok(_)) => true,
        Ok(Err(e)) => {
            tracing::warn!("Memory external freshness: failed to reindex {log_key}: {e}");
            false
        }
        Err(e) => {
            tracing::warn!("Memory external freshness: join failed for {log_key}: {e}");
            false
        }
    }
}

/// Spawned-once guard for the sweep loop.
static SWEEP_SPAWNED: AtomicBool = AtomicBool::new(false);
/// Set while a sweep tick is running, so an overlapping tick is skipped.
static SWEEP_RUNNING: AtomicBool = AtomicBool::new(false);

/// Spawn the background tier-2 external sweep (#1051, ADR-002).
///
/// Re-reads `[memory].sweep_interval_secs` every loop so a config change
/// applies live (Q15 — the sweep IS the reconciliation loop). Single-flight:
/// a tick that finds a sweep still running is skipped, not queued. No watcher,
/// no reload handler, no new concurrency surface — just a timer and a
/// metadata walk (ADR-002).
pub fn spawn_external_sweep() {
    if SWEEP_SPAWNED.swap(true, Ordering::AcqRel) {
        return;
    }
    tokio::spawn(async {
        loop {
            let secs = super::sweep_interval_secs().max(10);
            tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
            if SWEEP_RUNNING
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                continue;
            }
            sweep_once().await;
            SWEEP_RUNNING.store(false, Ordering::Release);
        }
    });
}

/// One sweep tick: run the incremental external walk under the store lock.
async fn sweep_once() {
    let store = match super::get_store() {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("Memory external sweep: store unavailable: {e}");
            return;
        }
    };
    let result = tokio::task::spawn_blocking(move || {
        let s = match store.lock() {
            Ok(s) => s,
            Err(e) => return Err(format!("Store lock poisoned: {e}")),
        };
        Ok::<_, String>(super::external_sweep::sweep_external(&s))
    })
    .await;
    match result {
        Ok(Ok(report)) => report.log(),
        Ok(Err(e)) => tracing::warn!("Memory external sweep failed: {e}"),
        Err(e) => tracing::warn!("Memory external sweep join failed: {e}"),
    }
}
