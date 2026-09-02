//! The memory section of a doctor report (#1067).

use super::health_report::{self, KeySource};
use super::keys::needs_embedding_key;
use super::settings::{read_memory_config, read_memory_section};
use super::store::get_store;
use crate::config::MemoryConfig;

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
fn key_source(cfg: &MemoryConfig) -> KeySource {
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
