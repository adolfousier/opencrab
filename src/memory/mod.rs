//! Memory Module
//!
//! Provides long-term memory search via our own SQLite FTS5 store
//! (`db.rs`) and vector semantic search (embeddinggemma-300M, local GGUF
//! via `local_engine.rs` or an OpenAI-compatible embedding API). Hybrid
//! RRF when embeddings are available, FTS-only fallback otherwise.
//!
//! When `config.memory.vector_enabled` is false, all vector/embedding code
//! is skipped — no model download, no llama.cpp init, FTS5-only search.

pub(crate) mod db;
pub(crate) mod embedding;
pub(crate) mod external;
pub(crate) mod external_sweep;
pub mod index;
pub(crate) mod local_engine;
pub(crate) mod search;
pub(crate) mod store;

pub use embedding::{
    embed_content, embed_content_api, embed_query_api, embed_via_api, engine_if_ready, get_engine,
};
pub(crate) mod chunk_fts;
pub(crate) mod chunker;
pub mod vector_search;

pub mod backfill_sweep;
pub mod freshness;
pub mod health_report;
pub use db::Store;
pub use index::{BRAIN_FILES, index_file, index_file_fts_only, reindex};
pub use search::{RrfResult, hybrid_search_rrf, search, search_brain};
pub(crate) use search::{search_external, search_memory};
pub use store::get_store;

/// Whether vector embeddings are enabled in the current config.
/// Reads `[memory].vector_enabled` from config.toml (default: true).
/// VPS/cloud auto-detection may set this to false.
fn vector_enabled() -> bool {
    let config = read_memory_config();
    config.vector_enabled
}

/// Read the `[memory]` section from config.toml, resolving the embedding
/// `api_key` from keys.toml when config.toml does not carry one.
///
/// `EmbeddingConfig::api_key` has always documented that it is "also loaded
/// from keys.toml under `[providers.memory_embedding]`", but nothing
/// implemented it, and the gap was two layers deep (#1066): `ProviderConfigs`
/// had no `memory_embedding` field, so serde dropped the section outright —
/// and even with a merge arm it could not have helped, because this function
/// parses config.toml directly and never passes through `Config::load()`.
/// Resolving the key here fixes it at the one place that actually consumes it,
/// and keeps the secret in keys.toml where the config/keys split intends it.
fn read_memory_config() -> crate::config::MemoryConfig {
    let mut cfg = read_memory_section().unwrap_or_default();
    if let Some(ref mut emb) = cfg.embedding
        && needs_embedding_key(emb.api_key.as_deref())
        && let Some(key) = memory_embedding_key_from_keys_file()
    {
        emb.api_key = Some(key);
    }
    cfg
}

/// Whether config.toml left the embedding key for keys.toml to supply.
///
/// A whitespace-only value counts as absent: it reaches the provider as an
/// empty bearer token and 401s exactly like no key at all, so treating it as
/// "configured" would keep the keys.toml fallback locked out for the one user
/// who most needs it.
pub(crate) fn needs_embedding_key(configured: Option<&str>) -> bool {
    configured.is_none_or(|k| k.trim().is_empty())
}

/// Parse just the `[memory]` table out of config.toml.
fn read_memory_section() -> Option<crate::config::MemoryConfig> {
    let config_path = crate::config::opencrabs_home().join("config.toml");
    let content = std::fs::read_to_string(&config_path).ok()?;
    let table = content.parse::<toml::Table>().ok()?;
    let memory = table.get("memory")?;
    toml::from_str::<crate::config::MemoryConfig>(&toml::to_string(memory).ok()?).ok()
}

/// `[providers.memory_embedding].api_key` from keys.toml, when it holds a real
/// credential. Mirrors the `__EXISTING_KEY__` sentinel guard that
/// `merge_provider_keys` applies, so the placeholder `/models` writes
/// internally is never mistaken for a key.
fn memory_embedding_key_from_keys_file() -> Option<String> {
    let keys_path = crate::config::opencrabs_home().join("keys.toml");
    let content = std::fs::read_to_string(&keys_path).ok()?;
    memory_embedding_key_in(&content)
}

/// The parsing half of the above, split out so the contract can be tested
/// without a keys.toml on disk.
///
/// The section is read as a raw toml table rather than through
/// `ProviderConfigs`, which has no `memory_embedding` field: adding one would
/// not help, because this module parses config.toml directly and never passes
/// through `Config::load()` where `merge_provider_keys` runs.
pub(crate) fn memory_embedding_key_in(keys_toml: &str) -> Option<String> {
    let table = keys_toml.parse::<toml::Table>().ok()?;
    let key = table
        .get("providers")?
        .get("memory_embedding")?
        .get("api_key")?
        .as_str()?;
    crate::config::stored_key::real_key(key).map(str::to_string)
}

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
fn embedding_api_config() -> Option<crate::config::EmbeddingConfig> {
    read_memory_config().embedding
}

/// The memory section of a doctor report, read from this install (#1067).
///
/// Deliberately resolves the key through `read_memory_config`, the same
/// function the embed path calls. Reporting a key from a different code path
/// than the one that consumes it is how #1066 stayed invisible: keys.toml held
/// a perfectly good credential that the runtime never saw.
pub fn doctor_lines() -> Vec<String> {
    let cfg = read_memory_config();
    let key = key_source(&cfg);
    let stats = get_store()
        .ok()
        .and_then(|s| s.lock().ok())
        .and_then(|s| s.vector_stats().ok());
    let health = crate::config::health::get_health(health_report::EMBEDDING_HEALTH_KEY);
    health_report::health_lines(
        &cfg,
        key,
        stats.as_ref(),
        health.as_ref(),
        chrono::Utc::now(),
    )
}

/// Which file supplied the embedding key, if any.
///
/// `cfg` has already been through the keys.toml fallback, so the raw section is
/// re-read to tell the two sources apart. Worth the second parse: "OK
/// (keys.toml)" is the line that proves the #1066 fallback is actually working
/// on this install.
fn key_source(cfg: &crate::config::MemoryConfig) -> health_report::KeySource {
    use health_report::KeySource;
    let Some(emb) = cfg.embedding.as_ref().filter(|e| e.url.is_some()) else {
        return KeySource::NotApplicable;
    };
    let in_config_toml = read_memory_section()
        .and_then(|c| c.embedding)
        .is_some_and(|e| !needs_embedding_key(e.api_key.as_deref()));
    if in_config_toml {
        return KeySource::ConfigToml;
    }
    if !needs_embedding_key(emb.api_key.as_deref()) {
        return KeySource::KeysToml;
    }
    KeySource::Missing
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

/// Extra external paths to index, from `[memory].extra_paths` (#1051).
/// Read fresh on every call — config changes settle within one sweep
/// interval with no restart or reload handler (the module live-reads
/// config by construction).
pub(crate) fn extra_paths_config() -> Vec<crate::config::ExtraPath> {
    read_memory_config().extra_paths
}

/// Global exclude patterns for external indexing (#1051).
pub(crate) fn external_excludes() -> Vec<String> {
    read_memory_config().exclude
}

/// Whether external results may surface in shared/group sessions (#1051).
/// Defaults to deny — the session gate is the security boundary.
pub(crate) fn external_allowed_in_shared() -> bool {
    read_memory_config().external_allowed_in_shared
}

/// Seconds between external freshness sweeps (#1051).
pub(crate) fn sweep_interval_secs() -> u64 {
    read_memory_config().sweep_interval_secs
}

/// Session IDs of shared/group channel sessions (#1051, ADR-003).
///
/// The external session gate must know whether the current session is a
/// shared/group chat (several people can read the reply) or the owner's own
/// session. Channel handlers know that when they resolve a session — they see
/// the chat type — so they mark it here; `memory_search` checks it before
/// returning external content. Process-local by design: on restart the set is
/// empty and each channel re-marks its group sessions on first use, which is
/// harmless because the gate only ever denies until then.
static SHARED_SESSIONS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashSet<uuid::Uuid>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashSet::new()));

/// Mark a session as a shared/group channel session (#1051). Called by the
/// channel handlers when they resolve a session for a group chat.
pub fn mark_session_shared(session_id: uuid::Uuid) {
    if let Ok(mut g) = SHARED_SESSIONS.lock() {
        g.insert(session_id);
    }
}

/// Whether a session is a shared/group channel session (#1051). Consulted by
/// the `memory_search` external gate.
pub fn is_session_shared(session_id: uuid::Uuid) -> bool {
    SHARED_SESSIONS
        .lock()
        .map(|g| g.contains(&session_id))
        .unwrap_or(false)
}

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
