//! Embedding gates derived from `[memory.embedding]`: whether an external
//! embedding API is configured, its resolved config, and the expected vector
//! dimensions.

use super::settings::read_memory_config;
use crate::config::EmbeddingConfig;

/// Whether an external embedding API is configured under `[memory.embedding]`.
pub(crate) fn embedding_api_configured() -> bool {
    let cfg = read_memory_config();
    // #1062: vector_enabled = false means no embedding work at all, local or
    // API. Folding the check here gives every caller (index Phase 2, memory
    // search) the gate in one place instead of each remembering to test it.
    cfg.vector_enabled
        && cfg
            .embedding
            .as_ref()
            .is_some_and(|e| e.url.is_some() && e.model.is_some())
}

/// Get the embedding API config if configured.
pub(crate) fn embedding_api_config() -> Option<EmbeddingConfig> {
    read_memory_config().embedding
}

/// Get the expected embedding dimensions.
/// Returns configured value, or 768 (local GGUF default).
pub(crate) fn embedding_dimensions() -> usize {
    let cfg = read_memory_config();
    if let Some(ref emb) = cfg.embedding
        && let Some(dims) = emb.dimensions
    {
        return dims;
    }
    768 // local GGUF embeddinggemma-300M default
}
