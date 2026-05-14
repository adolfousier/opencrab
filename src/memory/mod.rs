//! Memory Module
//!
//! Provides long-term memory search via the `qmd` crate's FTS5 engine and
//! vector semantic search (embeddinggemma-300M). Hybrid RRF when the model
//! is available, FTS-only fallback otherwise.
//!
//! When `config.memory.vector_enabled` is false, all vector/embedding code
//! is skipped — no model download, no llama.cpp init, FTS5-only search.

mod embedding;
mod index;
mod search;
mod store;

pub use embedding::{embed_content, engine_if_ready, get_engine};
pub use index::{BRAIN_FILES, index_file, reindex};
pub use search::search;
pub use store::get_store;

/// Whether vector embeddings are enabled in the current config.
/// Reads `[memory].vector_enabled` from config.toml (default: true).
/// VPS/cloud auto-detection may set this to false.
fn vector_enabled() -> bool {
    // Can't easily access the live Config here without passing it through
    // every call site, so read the raw config.toml directly.
    let config_path = crate::config::opencrabs_home().join("config.toml");
    if let Ok(content) = std::fs::read_to_string(&config_path)
        && let Ok(table) = content.parse::<toml::Table>()
        && let Some(memory) = table.get("memory").and_then(|m| m.as_table())
        && let Some(enabled) = memory.get("vector_enabled").and_then(|v| v.as_bool())
    {
        return enabled;
    }
    true // default: enabled
}

/// A single search result from the memory index.
#[derive(Debug, Clone)]
pub struct MemoryResult {
    pub path: String,
    pub snippet: String,
    pub rank: f64,
}

/// Collection name for daily compaction logs.
const COLLECTION_MEMORY: &str = "memory";
/// Collection name for workspace brain files (SOUL.md, MEMORY.md, etc.).
const COLLECTION_BRAIN: &str = "brain";
