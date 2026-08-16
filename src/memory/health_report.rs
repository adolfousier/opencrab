//! What `/doctor` says about memory embeddings (#1067).
//!
//! Memory could be completely broken and every diagnostic stayed green. An
//! embedding key that 401s does not stop search: the store falls back to
//! keyword-only FTS and keeps answering, so the only visible symptom is results
//! that feel slightly worse. One install ran 94 days with a single vectorised
//! chunk out of 589 before anyone looked.
//!
//! ## Why there is no live probe
//!
//! Nothing in this codebase probes a provider. The CLI doctor's provider check
//! constructs the client and never calls it; what every provider actually gets
//! is passive health recorded from real traffic (`config::health`). Embeddings
//! now feed the same table from the backfill sweep, which is strictly better
//! than a probe: it costs nothing, it says *since when* rather than *right
//! now*, and it refreshes every sweep tick instead of only when someone runs
//! doctor.
//!
//! ## Why formatting is separated from reading
//!
//! The two doctors (`slash_command::doctor_text` for channels,
//! `cli::commands::cmd_doctor` for the CLI) share no code and have already
//! drifted. Both call the same builder here, and the builder is pure so its
//! output can be asserted without a config.toml or a memory.db on disk.

use crate::config::MemoryConfig;
use crate::config::health::ProviderHealth;

use super::db::VectorStats;

/// Health-table key for the embedding endpoint. Shares the namespace with chat
/// providers deliberately: one table, one `/doctor` reader, one shape.
pub const EMBEDDING_HEALTH_KEY: &str = "memory_embedding";

/// Where the embedding API key came from, or why there is none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySource {
    /// No embedding API configured, so no key is expected.
    NotApplicable,
    /// An API is configured and no key was found anywhere. This is the failure
    /// that produced a silent 401 on every embed call.
    Missing,
    /// `[memory.embedding].api_key` in config.toml.
    ConfigToml,
    /// `[providers.memory_embedding].api_key` in keys.toml (#1066).
    KeysToml,
}

/// Whole-number days between an index timestamp and now.
///
/// `None` when the timestamp is absent or unparseable, so a store written by an
/// older schema degrades to "not shown" rather than to a wrong number.
pub fn days_since(timestamp: &str, now: chrono::DateTime<chrono::Utc>) -> Option<i64> {
    let then = chrono::DateTime::parse_from_rfc3339(timestamp)
        .ok()?
        .with_timezone(&chrono::Utc);
    Some((now - then).num_days())
}

/// The memory block for a doctor report.
///
/// Pure: every input is a parameter, so the output can be asserted without
/// touching config.toml, keys.toml, or memory.db.
pub fn health_lines(
    cfg: &MemoryConfig,
    key: KeySource,
    stats: Option<&VectorStats>,
    health: Option<&ProviderHealth>,
    now: chrono::DateTime<chrono::Utc>,
) -> Vec<String> {
    let mut lines = Vec::new();

    if !cfg.vector_enabled {
        // Deliberately terminal. Reporting a missing key for a feature that is
        // off on purpose is a false alarm, and #1062 made this a supported
        // configuration rather than a degraded one.
        lines.push("Vectors: disabled (vector_enabled = false, FTS-only search)".to_string());
        return lines;
    }

    match cfg.embedding.as_ref().filter(|e| e.url.is_some()) {
        Some(emb) => {
            let model = emb.model.as_deref().unwrap_or("(no model set)");
            lines.push(format!("Vectors: enabled (API: {model})"));
            if let Some(url) = emb.url.as_deref() {
                lines.push(format!("Endpoint: {url}"));
            }
            lines.push(format!("Embedding key: {}", key_line(key)));
        }
        None => {
            lines.push("Vectors: enabled (local GGUF model)".to_string());
        }
    }

    if let Some(s) = stats {
        lines.push(format!(
            "Documents: {} indexed, {} awaiting embedding",
            s.documents_active, s.documents_unembedded
        ));
        lines.push(format!("Chunks embedded: {}", s.vector_rows));
        match s.last_embedded_at.as_deref() {
            Some(ts) => {
                let age = days_since(ts, now)
                    .map(|d| format!(" ({d} days ago)"))
                    .unwrap_or_default();
                lines.push(format!("Last embedded: {ts}{age}"));
            }
            // Not the same as an old date. Never means the backfill has not
            // completed once on this install, which is the #1069 shape.
            None => lines.push("Last embedded: never".to_string()),
        }
    }

    if let Some(h) = health {
        lines.push(format!("Embedding API: {}", api_health_line(h)));
    }

    lines
}

fn key_line(key: KeySource) -> String {
    match key {
        KeySource::NotApplicable => "n/a (local model)".to_string(),
        KeySource::Missing => "MISSING (embed calls will fail with 401)".to_string(),
        KeySource::ConfigToml => "OK (config.toml)".to_string(),
        KeySource::KeysToml => "OK (keys.toml)".to_string(),
    }
}

fn api_health_line(h: &ProviderHealth) -> String {
    if h.consecutive_failures > 0 {
        let err = h.last_error.as_deref().unwrap_or("no error recorded");
        return format!("FAILING ({}x): {err}", h.consecutive_failures);
    }
    if h.last_success.is_some() {
        return "OK".to_string();
    }
    "no calls recorded yet".to_string()
}
