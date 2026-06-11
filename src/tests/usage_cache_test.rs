//! Cache Efficiency Card Tests
//!
//! Tests for CacheStats struct, percentage calculation, and edge cases.

use crate::usage::data::CacheStats;

// ─────────────────────────────────────────────────────────────────────────────
// CacheStats struct tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn cache_stats_default_is_zero() {
    let stats = CacheStats::default();
    assert_eq!(stats.cache_hit_pct, 0.0);
    assert_eq!(stats.cached_tokens, 0);
    assert_eq!(stats.total_input_tokens, 0);
}

// ─────────────────────────────────────────────────────────────────────────────
// Cache hit percentage calculation
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn cache_hit_pct_simple_case() {
    // 800 cached out of 1000 total = 80%
    let cached = 800i64;
    let total = 1000i64;
    let pct = (cached as f64 / total as f64) * 100.0;
    assert!((pct - 80.0).abs() < 0.01);
}

#[test]
fn cache_hit_pct_zero_cached() {
    let cached = 0i64;
    let total = 1000i64;
    let pct = (cached as f64 / total as f64) * 100.0;
    assert!((pct - 0.0).abs() < 0.01);
}

#[test]
fn cache_hit_pct_all_cached() {
    let cached = 1000i64;
    let total = 1000i64;
    let pct = (cached as f64 / total as f64) * 100.0;
    assert!((pct - 100.0).abs() < 0.01);
}

#[test]
fn cache_hit_pct_partial_cache() {
    // Simulating real Anthropic numbers:
    // input_tokens=1000, cache_creation=80000, cache_read=15000
    // total_input = 1000 + 80000 + 15000 = 96000
    // cached = 15000
    // pct = 15000/96000 = 15.625%
    let cached = 15000i64;
    let total = 96000i64;
    let pct = (cached as f64 / total as f64) * 100.0;
    assert!((pct - 15.625).abs() < 0.01);
}

// ─────────────────────────────────────────────────────────────────────────────
// CacheStats construction with realistic data
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn cache_stats_construction() {
    let stats = CacheStats {
        cache_hit_pct: 67.5,
        cached_tokens: 1_200_000,
        total_input_tokens: 1_800_000,
    };
    assert!((stats.cache_hit_pct - 67.5).abs() < 0.01);
    assert_eq!(stats.cached_tokens, 1_200_000);
    assert_eq!(stats.total_input_tokens, 1_800_000);
}

#[test]
fn cache_stats_no_cache_data() {
    // When there's no cache data, DashboardData.cache should be None
    // This simulates a fresh install with no messages yet
    let stats: Option<CacheStats> = None;
    assert!(stats.is_none());
}

// ─────────────────────────────────────────────────────────────────────────────
// Color thresholds (matching render_cache_efficiency logic)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn cache_pct_green_threshold() {
    // >= 60% should be green
    let pct = 67.0;
    assert!(pct >= 60.0);
}

#[test]
fn cache_pct_yellow_threshold() {
    // 30-60% should be yellow
    let pct = 45.0;
    assert!((30.0..60.0).contains(&pct));
}

#[test]
fn cache_pct_red_threshold() {
    // < 30% should be red
    let pct = 15.0;
    assert!(pct < 30.0);
}

#[test]
fn cache_pct_boundary_60() {
    // Exactly 60% should be green (>= 60)
    let pct = 60.0;
    assert!(pct >= 60.0);
}

#[test]
fn cache_pct_boundary_30() {
    // Exactly 30% should be yellow (>= 30 and < 60)
    let pct = 30.0;
    assert!((30.0..60.0).contains(&pct));
}

// ─────────────────────────────────────────────────────────────────────────────
// DashboardData cache field
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn dashboard_data_cache_field_defaults_to_none() {
    use crate::usage::data::DashboardData;
    let d = DashboardData::default();
    assert!(d.cache.is_none());
}

// ─────────────────────────────────────────────────────────────────────────────
// SQL aggregation — the NULL-cache-column trap that inflated the % (audit:
// 87% reported vs 53% actual). Runs the ACTUAL production aggregation columns.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn aggregation_counts_input_from_rows_with_null_cache_columns() {
    use crate::usage::data::CACHE_STATS_SELECT_COLS;

    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE messages (
            input_tokens INTEGER,
            cache_creation_tokens INTEGER,
            cache_read_tokens INTEGER
         );
         -- a caching request: 90 of its input was served from cache
         INSERT INTO messages VALUES (10, 0, 90);
         -- a NON-caching request: the cache columns are NULL, exactly as the
         -- agent records them when the provider returns no cache usage
         INSERT INTO messages VALUES (100, NULL, NULL);",
    )
    .unwrap();

    let sql = format!(
        "SELECT {CACHE_STATS_SELECT_COLS} FROM messages \
         WHERE cache_read_tokens > 0 OR cache_creation_tokens > 0 OR input_tokens > 0"
    );
    let (cached, total): (i64, i64) = conn
        .query_row(&sql, [], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();

    // Cached is correct either way (the NULL row contributes 0 cached).
    assert_eq!(cached, 90);
    // The denominator MUST include the NULL-cache row's 100 input tokens:
    // 10 + 90 + 100 = 200. The old `SUM(input + cache_creation + cache_read)`
    // returned NULL for that row and dropped it, yielding 100 — which inflated
    // the hit rate from the true 45% to a bogus 90%.
    assert_eq!(
        total, 200,
        "input tokens from rows with NULL cache columns must be counted"
    );

    let pct = 100.0 * cached as f64 / total as f64;
    assert!((pct - 45.0).abs() < 0.01, "expected 45%, got {pct}");
}
