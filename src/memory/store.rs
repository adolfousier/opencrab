//! Store — singleton qmd Store for the memory database.

use once_cell::sync::OnceCell;
use qmd::Store;
use std::path::PathBuf;
use std::sync::Mutex;

static STORE: OnceCell<Mutex<Store>> = OnceCell::new();

/// Get (or create) the shared memory qmd Store.
///
/// The database lives at `~/.opencrabs/memory/memory.db`.
/// First call initializes the schema via `Store::open` and creates the vector table
/// (only when vector embeddings are enabled in config).
pub fn get_store() -> Result<&'static Mutex<Store>, String> {
    STORE.get_or_try_init(|| {
        let db_path = memory_dir().join("memory.db");

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
        Ok(Mutex::new(store))
    })
}

/// Path to the memory directory: `~/.opencrabs/memory/`
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
