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

pub use embedding::{
    embed_content, embed_content_api, embed_query_api, embed_via_api, engine_if_ready, get_engine,
};
pub use index::{BRAIN_FILES, index_file, reindex};
pub use search::search;
pub use store::get_store;

/// Whether vector embeddings are enabled in the current config.
/// Reads `[memory].vector_enabled` from config.toml (default: true).
/// VPS/cloud auto-detection may set this to false.
fn vector_enabled() -> bool {
    let config = read_memory_config();
    config.vector_enabled
}

/// Read the `[memory]` section from config.toml.
fn read_memory_config() -> crate::config::MemoryConfig {
    let config_path = crate::config::opencrabs_home().join("config.toml");
    if let Ok(content) = std::fs::read_to_string(&config_path)
        && let Ok(table) = content.parse::<toml::Table>()
        && let Some(memory) = table.get("memory")
        && let Ok(cfg) = toml::from_str::<crate::config::MemoryConfig>(
            &toml::to_string(memory).unwrap_or_default(),
        )
    {
        return cfg;
    }
    crate::config::MemoryConfig::default()
}

/// Whether an external embedding API is configured under `[memory.embedding]`.
fn embedding_api_configured() -> bool {
    let cfg = read_memory_config();
    cfg.embedding
        .as_ref()
        .is_some_and(|e| e.url.is_some() && e.model.is_some())
}

/// Get the embedding API config if configured.
fn embedding_api_config() -> Option<crate::config::EmbeddingConfig> {
    read_memory_config().embedding
}

/// Get the expected embedding dimensions.
/// Returns configured value, or 768 (local GGUF default).
fn embedding_dimensions() -> usize {
    let cfg = read_memory_config();
    if let Some(ref emb) = cfg.embedding
        && let Some(dims) = emb.dimensions
    {
        return dims;
    }
    768 // local GGUF embeddinggemma-300M default
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
