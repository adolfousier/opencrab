//! Unit tests for Claude CLI provider model normalization.
//!
//! Covers the date-suffix stripper and the alias → normalized-id map so
//! renames/release bumps are caught by CI instead of visually in the TUI.

use crate::brain::provider::claude_cli::{
    ClaudeCliProvider, clear_learned_models, learned_alias, record_alias, strip_claude_date_suffix,
};

/// Serializes the tests that mutate the process-wide learned-model cache,
/// so `clear_learned_models()` in one can't wipe a key another is asserting.
///
/// This must be the SHARED `crate::tests::MODEL_CACHE_LOCK`, not a lock private
/// to this file: a private one serialized the mutators against each other but
/// not against the tests that only read the cache, which is how a reader
/// comparing two evaluations of discovery saw a clear() land between them
/// (#932).
fn cache_lock() -> std::sync::MutexGuard<'static, ()> {
    crate::tests::MODEL_CACHE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[test]
fn strip_claude_date_suffix_strips_eight_digit_date() {
    assert_eq!(
        strip_claude_date_suffix("claude-opus-4-7-20260115"),
        "claude-opus-4-7"
    );
    assert_eq!(strip_claude_date_suffix("opus-4-7-20260115"), "opus-4-7");
}

#[test]
fn strip_claude_date_suffix_leaves_non_date_alone() {
    assert_eq!(strip_claude_date_suffix("opus-4-7"), "opus-4-7");
    assert_eq!(strip_claude_date_suffix("opus"), "opus");
    assert_eq!(
        strip_claude_date_suffix("sonnet-4-6-beta"),
        "sonnet-4-6-beta"
    );
}

#[test]
fn seed_for_alias_holds_the_build_time_fallbacks() {
    // These are the seeds used only until the CLI reports the live
    // release on the first turn. Asserting the pure seed function keeps
    // this test independent of the process-wide learned cache (which
    // other tests mutate).
    assert_eq!(ClaudeCliProvider::seed_for_alias("opus"), Some("opus-4-7"));
    assert_eq!(
        ClaudeCliProvider::seed_for_alias("sonnet"),
        Some("sonnet-4-6")
    );
    assert_eq!(
        ClaudeCliProvider::seed_for_alias("haiku"),
        Some("haiku-4-5")
    );
    // Anything that is not a bare alias has no seed (passed through by callers).
    assert_eq!(ClaudeCliProvider::seed_for_alias("opus-4-9"), None);
}

#[test]
fn learned_version_overrides_the_seed() {
    let _guard = cache_lock();
    // Simulate the CLI resolving `opus` to a release newer than the seed.
    // After observing it, the alias must resolve to the learned version so
    // footers/pricing track Anthropic's current model with no code change.
    clear_learned_models();
    assert_eq!(
        ClaudeCliProvider::default_for_alias("opus"),
        "opus-4-7",
        "with an empty cache, the seed is used"
    );
    ClaudeCliProvider::record_observed_model("claude-opus-4-8");
    assert_eq!(learned_alias("opus").as_deref(), Some("opus-4-8"));
    assert_eq!(
        ClaudeCliProvider::default_for_alias("opus"),
        "opus-4-8",
        "the learned version must win over the build-time seed"
    );
    // A bare alias now normalizes to the learned version too.
    assert_eq!(ClaudeCliProvider::normalize_model("opus"), "opus-4-8");
    // Clean up so we don't leak state into other tests sharing the cache.
    clear_learned_models();
}

#[test]
fn record_alias_is_idempotent_and_updates() {
    let _guard = cache_lock();
    clear_learned_models();
    record_alias("haiku", "haiku-4-5");
    assert_eq!(learned_alias("haiku").as_deref(), Some("haiku-4-5"));
    // Recording the same value again is a no-op; a new value replaces it.
    record_alias("haiku", "haiku-4-5");
    record_alias("haiku", "haiku-4-6");
    assert_eq!(learned_alias("haiku").as_deref(), Some("haiku-4-6"));
    clear_learned_models();
}

#[test]
fn normalize_model_strips_claude_prefix_and_date() {
    assert_eq!(
        ClaudeCliProvider::normalize_model("claude-opus-4-7-20260115"),
        "opus-4-7"
    );
    assert_eq!(
        ClaudeCliProvider::normalize_model("claude-sonnet-4-6-20251112"),
        "sonnet-4-6"
    );
}

#[test]
fn normalize_model_passes_through_unknown() {
    assert_eq!(
        ClaudeCliProvider::normalize_model("opus-4-8"),
        "opus-4-8",
        "a future release shorthand should flow through unchanged"
    );
    assert_eq!(
        ClaudeCliProvider::normalize_model("custom-model-id"),
        "custom-model-id"
    );
}
