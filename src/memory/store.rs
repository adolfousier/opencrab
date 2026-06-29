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
