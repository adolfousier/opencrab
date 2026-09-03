//! Shared types and collection names for the memory index.

/// A single search result from the memory index.
#[derive(Debug, Clone)]
pub struct MemoryResult {
    pub path: String,
    pub snippet: String,
    pub rank: f64,
}

/// Collection name for daily compaction logs.
pub(crate) const COLLECTION_MEMORY: &str = "memory";
/// Collection name for workspace brain files (SOUL.md, MEMORY.md, etc.).
pub(crate) const COLLECTION_BRAIN: &str = "brain";
/// Collection name for user-configured external paths (#1051). Keyed by
/// absolute canonical path — unlike brain/memory, which key by basename.
pub(crate) const COLLECTION_EXTERNAL: &str = "external";
