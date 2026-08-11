//! Store — per-profile qmd Stores for the memory database.
//!
//! Keyed by resolved database path, NOT a single global (#999). This was one
//! `OnceCell` that captured `opencrabs_home()` on the first call and cached the
//! Store forever. Profiles genuinely switch inside one process: the cron
//! scheduler runs each job inside `with_profile_home_async`, so a job under
//! profile B executed with B's config, keys and brain files while reading and
//! writing profile A's `memory.db`, whichever profile happened to initialize
//! first. Since the turn path indexes MEMORY.md, that wrote one profile's
//! memory into another's index.
//!
//! Profiles are the isolation boundary in this codebase, so the component
//! holding indexed content has to respect it.

use qmd::Store;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

/// Open stores, one per database path.
///
/// Values are leaked to keep the `&'static` return that callers rely on. That
/// is bounded and intentional: one entry per profile actually used in this
/// process, each of which would live for the process lifetime anyway.
static STORES: LazyLock<Mutex<HashMap<PathBuf, &'static Mutex<Store>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Get (or create) the memory qmd Store for the ACTIVE profile.
///
/// The database lives at `<profile home>/memory/memory.db`, resolved on every
/// call so a profile switch reaches the right file. First use of a given path
/// initializes the schema via `Store::open` and creates the vector table (only
/// when vector embeddings are enabled in config).
pub fn get_store() -> Result<&'static Mutex<Store>, String> {
    let db_path = memory_dir().join("memory.db");

    // Fast path: already open for this profile.
    {
        let map = STORES
            .lock()
            .map_err(|e| format!("Store registry lock poisoned: {e}"))?;
        if let Some(store) = map.get(&db_path) {
            return Ok(*store);
        }
    }

    let store = open_store(&db_path)?;

    let mut map = STORES
        .lock()
        .map_err(|e| format!("Store registry lock poisoned: {e}"))?;
    // Another thread may have opened it while this one was building. Keep
    // theirs and drop ours rather than leaking a second handle to one file.
    Ok(map.entry(db_path).or_insert(store))
}

/// Open one store and leak it, so the handle can be `&'static`.
fn open_store(db_path: &Path) -> Result<&'static Mutex<Store>, String> {
    let store = build_store(db_path)?;
    Ok(Box::leak(Box::new(Mutex::new(store))))
}

fn build_store(db_path: &Path) -> Result<Store, String> {
    {
        let db_path = db_path.to_path_buf();

        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create memory dir: {e}"))?;
        }

        let store =
            Store::open(&db_path).map_err(|e| format!("Failed to open memory store: {e}"))?;

        // Only create vector table when embeddings are enabled
        if super::vector_enabled() {
            let dims = super::embedding_dimensions();
            store
                .ensure_vector_table(dims)
                .map_err(|e| format!("Failed to create vector table: {e}"))?;
            tracing::info!("Vector table created with {dims} dimensions");
        }

        tracing::info!(
            "Memory qmd store ready at {} (vector: {})",
            db_path.display(),
            if super::vector_enabled() {
                "enabled"
            } else {
                "disabled"
            }
        );
        Ok(store)
    }
}

/// Path to the memory directory: `<profile home>/memory/`
pub(crate) fn memory_dir() -> PathBuf {
    crate::config::opencrabs_home().join("memory")
}

/// Delete the `skipped-too-large` placeholder rows left by the pre-chunking
/// embedder, so those documents become eligible again (#998).
///
/// The old path wrote a zero-length embedding for anything over 32 KB
/// specifically so it would stop retrying. That was correct while documents
/// were embedded whole, and is exactly wrong now: with chunking, no chunk
/// approaches the limit, so those documents CAN be embedded and the
/// placeholders are the only thing preventing it. `get_hashes_needing_embedding`
/// keys on the presence of a `seq = 0` row, so a placeholder makes a document
/// invisible to backfill forever.
///
/// Idempotent: deletes nothing once the sweep has run. Uses its own read-write
/// connection because `qmd::Store` exposes no raw statement access; WAL plus a
/// busy timeout makes that safe alongside the store's own connection.
pub(crate) fn clear_skipped_placeholders() -> Result<usize, String> {
    let db_path = memory_dir().join("memory.db");
    if !db_path.exists() {
        return Ok(0);
    }

    let conn = rusqlite::Connection::open(&db_path)
        .map_err(|e| format!("Failed to open store for placeholder sweep: {e}"))?;
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| format!("Failed to set busy timeout: {e}"))?;

    // Drop the vector blobs first: content_vectors is what identifies them, so
    // removing it first would orphan the blobs with no way to find them again.
    conn.execute(
        "DELETE FROM vectors_vec WHERE hash_seq IN (
             SELECT hash || '_' || seq FROM content_vectors WHERE model = 'skipped-too-large'
         )",
        [],
    )
    .map_err(|e| format!("Failed to clear placeholder vectors: {e}"))?;

    let removed = conn
        .execute(
            "DELETE FROM content_vectors WHERE model = 'skipped-too-large'",
            [],
        )
        .map_err(|e| format!("Failed to clear placeholder rows: {e}"))?;

    if removed > 0 {
        tracing::info!(
            "Cleared {removed} skipped-too-large embedding placeholders; \
             those documents will be chunked and embedded on the next backfill"
        );
    }
    Ok(removed)
}
