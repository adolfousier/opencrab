//! Tests for `is_cosmetic_alias_of_parent` — the noise-suppressor
//! that stops the /usage breakdown from rendering trivial-alias
//! variants under their canonical parent group.
//!
//! Regression context: 2026-05-30 user screenshot showed
//! `qwen-3.7-max` (parent) with `qwen3.7-max` (child, no hyphen
//! between `qwen` and `3.7`) listed as a separate variant under it.
//! Same model, different cosmetic separator — listing both is pure
//! noise. The SQL normalization already collapses them into the
//! same canonical parent for the total row, but the breakdown
//! tree still rendered the raw variant.

use crate::usage::data::is_cosmetic_alias_of_parent;

#[test]
fn no_separator_vs_hyphen_separator_is_alias() {
    // The exact incident from the screenshot.
    assert!(is_cosmetic_alias_of_parent("qwen3.7-max", "qwen-3.7-max"));
    assert!(is_cosmetic_alias_of_parent("qwen-3.7-max", "qwen3.7-max"));
}

#[test]
fn dotted_vs_dashed_version_is_alias() {
    // `qwen-3-7-max` (dash separators on version) vs the canonical
    // `qwen-3.7-max` (dot). Both surface forms exist in the wild.
    assert!(is_cosmetic_alias_of_parent("qwen-3-7-max", "qwen-3.7-max"));
    assert!(is_cosmetic_alias_of_parent("qwen3-7-max", "qwen-3.7-max"));
}

#[test]
fn display_name_with_spaces_is_alias() {
    // The model registry sometimes records the human-readable
    // display name. Same model, just formatted for a label.
    assert!(is_cosmetic_alias_of_parent("Qwen 3.7 Max", "qwen-3.7-max"));
    assert!(is_cosmetic_alias_of_parent("QWEN 3.7 MAX", "qwen-3.7-max"));
}

#[test]
fn preview_variant_is_not_an_alias() {
    // -preview is a meaningfully different model (different
    // weights, different API endpoint). Must NOT collapse.
    assert!(!is_cosmetic_alias_of_parent(
        "qwen-3.7-max-preview",
        "qwen-3.7-max"
    ));
    assert!(!is_cosmetic_alias_of_parent(
        "qwen3.7-max-preview",
        "qwen-3.7-max"
    ));
}

#[test]
fn dated_snapshot_is_not_an_alias() {
    // -20260520 is a pinned snapshot. Even if the SQL groups it
    // under qwen-3.7-max for the total row, the breakdown should
    // still surface it because seeing "30% of spend was on the
    // dated snapshot" is useful operational info.
    assert!(!is_cosmetic_alias_of_parent(
        "qwen-3.7-max-20260520",
        "qwen-3.7-max"
    ));
    assert!(!is_cosmetic_alias_of_parent(
        "qwen3.7-max-20260520",
        "qwen-3.7-max"
    ));
}

#[test]
fn latest_series_invite_beta_is_not_an_alias() {
    // The `qwen-latest-series-invite-beta-v34` channel has its own
    // canonical form. Showing it as a variant of qwen-3.7-max is
    // useful — that's how the user knows what's funding the spend.
    assert!(!is_cosmetic_alias_of_parent(
        "qwen-latest-series-invite-beta-v34",
        "qwen-3.7-max"
    ));
}

#[test]
fn empty_strings_compare_equal() {
    // Defensive: both empty → canonically same. The caller
    // shouldn't be invoking with empty model names, but the
    // function should not panic.
    assert!(is_cosmetic_alias_of_parent("", ""));
}

#[test]
fn whitespace_only_collapses_to_empty() {
    // `"   "` and `""` canonicalize to the same empty string.
    assert!(is_cosmetic_alias_of_parent("   ", ""));
}

#[test]
fn unrelated_models_are_not_aliases() {
    assert!(!is_cosmetic_alias_of_parent("opus-4-6", "qwen-3.7-max"));
    assert!(!is_cosmetic_alias_of_parent(
        "qwen-3.6-plus",
        "qwen-3.7-max"
    ));
    assert!(!is_cosmetic_alias_of_parent("kimi-k2.6", "qwen-3.7-max"));
}

#[test]
fn version_family_difference_is_not_an_alias() {
    // `qwen-3.6-max` vs `qwen-3.7-max` — different version
    // families. MUST NOT collapse even though the prefix matches.
    assert!(!is_cosmetic_alias_of_parent("qwen-3.6-max", "qwen-3.7-max"));
    assert!(!is_cosmetic_alias_of_parent("qwen3.6-max", "qwen-3.7-max"));
}

#[test]
fn underscore_vs_dash_is_alias() {
    // Some model registries use underscores. Same canonical form
    // since both `_` and `-` are non-alphanumeric.
    assert!(is_cosmetic_alias_of_parent("qwen_3_7_max", "qwen-3.7-max"));
}

#[test]
fn slash_separator_is_alias() {
    // Provider-prefixed forms like `qwen/3.7-max` should also
    // canonicalize the same.
    assert!(is_cosmetic_alias_of_parent("qwen/3.7-max", "qwen-3.7-max"));
}

#[test]
fn case_only_difference_is_alias() {
    assert!(is_cosmetic_alias_of_parent("Qwen-3.7-Max", "qwen-3.7-max"));
}

// ── Group key (#929) ──────────────────────────────────────────────────────

use crate::usage::data::canonical_model_key;

/// The spellings that split one model into two top-level rows, each carrying
/// its own spend, so the model's real total was never shown anywhere.
#[test]
fn punctuation_variants_share_a_group_key() {
    assert_eq!(
        canonical_model_key("qwen3.8-max-preview"),
        canonical_model_key("qwen-3.8-max-preview"),
        "a hyphen after the vendor prefix must not split the model"
    );
}

#[test]
fn case_and_separators_collapse() {
    let expect = canonical_model_key("qwen-3.7-max");
    for variant in ["Qwen 3.7 Max", "qwen3.7max", "QWEN_3_7_MAX", "qwen.3.7.max"] {
        assert_eq!(
            canonical_model_key(variant),
            expect,
            "{variant} should match"
        );
    }
}

/// The over-merge guard. These differ in alphanumerics, not just punctuation,
/// so collapsing them would report distinct models as one.
#[test]
fn genuinely_different_models_keep_distinct_keys() {
    let base = canonical_model_key("qwen-3.8-max");
    for other in [
        "qwen-3.8-max-preview", // preview is a different model
        "qwen-3.7-max",         // different version
        "qwen3.8-max-20260520", // dated snapshot
        "qwen-3.8-plus",        // different tier
    ] {
        assert_ne!(
            canonical_model_key(other),
            base,
            "{other} must stay separate"
        );
    }
}

/// The key exists to group, never to display: it is unreadable on purpose, so
/// the caller has to carry a display name alongside it.
#[test]
fn key_strips_everything_but_alphanumerics() {
    assert_eq!(
        canonical_model_key("qwen-3.8-max-preview"),
        "qwen38maxpreview"
    );
}
