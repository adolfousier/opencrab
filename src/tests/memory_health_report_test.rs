//! What `/doctor` reports about memory embeddings (#1067).
//!
//! A broken embedding key never surfaced anywhere: search falls back to
//! keyword-only FTS and keeps answering, so the only symptom is results that
//! feel slightly worse. One install ran 94 days with a single vectorised chunk
//! out of 589 with every diagnostic green.
//!
//! Fixtures are synthetic and carry no real credentials or endpoints.

use crate::config::health::ProviderHealth;
use crate::config::{EmbeddingConfig, MemoryConfig};
use crate::memory::db::VectorStats;
use crate::memory::health_report::{KeySource, days_since, health_lines};

fn now() -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339("2026-08-16T12:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc)
}

fn api_cfg() -> MemoryConfig {
    MemoryConfig {
        vector_enabled: true,
        embedding: Some(EmbeddingConfig {
            url: Some("https://embeddings.invalid/v1".to_string()),
            model: Some("text-embedding-test".to_string()),
            api_key: Some("sk-test".to_string()),
            dimensions: None,
        }),
        backfill_interval_secs: 300,
        ..Default::default()
    }
}

fn stale_stats() -> VectorStats {
    // The real shape of the install that motivated this: 66 documents, one
    // vectorised chunk, last embedded three months ago.
    VectorStats {
        documents_active: 66,
        documents_unembedded: 65,
        vector_rows: 1,
        last_embedded_at: Some("2026-05-14T09:00:00Z".to_string()),
    }
}

fn joined(lines: Vec<String>) -> String {
    lines.join("\n")
}

#[test]
fn a_configured_api_reports_its_model_and_endpoint() {
    let out = joined(health_lines(
        &api_cfg(),
        KeySource::ConfigToml,
        None,
        None,
        now(),
    ));
    assert!(
        out.contains("Vectors: enabled (API: text-embedding-test)"),
        "{out}"
    );
    assert!(
        out.contains("Endpoint: https://embeddings.invalid/v1"),
        "{out}"
    );
    assert!(out.contains("Embedding key: OK (config.toml)"), "{out}");
}

#[test]
fn a_key_from_keys_toml_is_named_as_such() {
    // The #1066 fallback. Saying which file supplied it is the only way to see
    // from a doctor run that the fallback is actually working on this install.
    let out = joined(health_lines(
        &api_cfg(),
        KeySource::KeysToml,
        None,
        None,
        now(),
    ));
    assert!(out.contains("Embedding key: OK (keys.toml)"), "{out}");
}

#[test]
fn a_missing_key_says_what_will_happen() {
    // "MISSING" alone reads as cosmetic. Naming the 401 connects it to the
    // symptom someone is already looking at.
    let out = joined(health_lines(
        &api_cfg(),
        KeySource::Missing,
        None,
        None,
        now(),
    ));
    assert!(out.contains("Embedding key: MISSING"), "{out}");
    assert!(out.contains("401"), "{out}");
}

#[test]
fn disabled_vectors_report_no_key_alarm() {
    // #1062 made this a supported configuration. Reporting MISSING for a
    // feature that is off on purpose is a false alarm, and a doctor that cries
    // wolf is a doctor nobody reads.
    let cfg = MemoryConfig {
        vector_enabled: false,
        ..api_cfg()
    };
    let out = joined(health_lines(
        &cfg,
        KeySource::Missing,
        Some(&stale_stats()),
        None,
        now(),
    ));
    assert!(out.contains("Vectors: disabled"), "{out}");
    assert!(
        !out.contains("MISSING"),
        "must not alarm on a disabled feature: {out}"
    );
    assert!(!out.contains("awaiting embedding"), "{out}");
}

#[test]
fn a_local_model_needs_no_key_line() {
    let cfg = MemoryConfig {
        vector_enabled: true,
        embedding: None,
        backfill_interval_secs: 300,
        ..Default::default()
    };
    let out = joined(health_lines(
        &cfg,
        KeySource::NotApplicable,
        None,
        None,
        now(),
    ));
    assert!(out.contains("Vectors: enabled (local GGUF model)"), "{out}");
    assert!(!out.contains("Endpoint:"), "{out}");
}

#[test]
fn the_stalled_backfill_is_visible_in_the_counts() {
    // The two numbers that would have made #1069 self-evident.
    let out = joined(health_lines(
        &api_cfg(),
        KeySource::KeysToml,
        Some(&stale_stats()),
        None,
        now(),
    ));
    assert!(
        out.contains("Documents: 66 indexed, 65 awaiting embedding"),
        "{out}"
    );
    assert!(out.contains("Chunks embedded: 1"), "{out}");
    assert!(out.contains("94 days ago"), "{out}");
}

#[test]
fn never_embedded_is_distinct_from_an_old_date() {
    // Not the same condition: "never" means the backfill has not completed once
    // on this install, which is exactly the #1069 shape on a fresh box.
    let stats = VectorStats {
        documents_active: 12,
        documents_unembedded: 12,
        vector_rows: 0,
        last_embedded_at: None,
    };
    let out = joined(health_lines(
        &api_cfg(),
        KeySource::Missing,
        Some(&stats),
        None,
        now(),
    ));
    assert!(out.contains("Last embedded: never"), "{out}");
}

#[test]
fn passive_health_replaces_a_live_probe() {
    // No provider in this codebase is probed; they are all judged on recorded
    // traffic. The embedding endpoint now feeds the same table, which says
    // since when rather than right now, and costs no API call.
    let failing = ProviderHealth {
        last_success: None,
        last_failure: Some(1),
        last_error: Some("401 invalid api key".to_string()),
        consecutive_failures: 27,
    };
    let out = joined(health_lines(
        &api_cfg(),
        KeySource::KeysToml,
        None,
        Some(&failing),
        now(),
    ));
    assert!(
        out.contains("Embedding API: FAILING (27x): 401 invalid api key"),
        "{out}"
    );

    let healthy = ProviderHealth {
        last_success: Some(1),
        last_failure: None,
        last_error: None,
        consecutive_failures: 0,
    };
    let out = joined(health_lines(
        &api_cfg(),
        KeySource::KeysToml,
        None,
        Some(&healthy),
        now(),
    ));
    assert!(out.contains("Embedding API: OK"), "{out}");
}

#[test]
fn an_unparseable_timestamp_degrades_to_no_age() {
    // A store written by an older schema must show the raw value rather than a
    // wrong number of days.
    assert_eq!(days_since("not a timestamp", now()), None);
    assert_eq!(days_since("2026-08-16T12:00:00Z", now()), Some(0));
    assert_eq!(days_since("2026-08-06T12:00:00Z", now()), Some(10));
}
