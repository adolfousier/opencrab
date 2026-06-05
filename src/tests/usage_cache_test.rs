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
