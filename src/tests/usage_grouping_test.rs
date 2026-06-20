//! Tests for `normalize_model_for_grouping` — pin the parent-bucket
//! every quantization variant collapses into for the /usage dashboard.
//!
//! Regression anchor: 2026-05-17 screenshot showed
//! `qwen3.6-35b-a3b-ud-iq4_xs.gguf` rendering as its own top-level row
//! ($6.52, 15.5M) instead of folding under `qwen3.6-35b-a3b` alongside
//! `-gguf`, `-oq2`, `-oq4`. Root cause: the model name ended in `.gguf`
//! (file extension with a dot), and the quant patterns only matched
//! `-gguf` / `-ud-iq4_xs` / etc. so nothing stripped.

use crate::usage::data::{normalize_model_for_grouping, sql_normalize_model};

#[test]
fn strips_dot_gguf_then_quant_tag() {
    // The exact case from the screenshot.
    assert_eq!(
        normalize_model_for_grouping("qwen3.6-35b-a3b-ud-iq4_xs.gguf"),
        "qwen3.6-35b-a3b"
    );
}

#[test]
fn strips_bare_dash_gguf() {
    assert_eq!(
        normalize_model_for_grouping("qwen3.6-35b-a3b-gguf"),
        "qwen3.6-35b-a3b"
    );
}

#[test]
fn strips_oq_quant_variants() {
    assert_eq!(
        normalize_model_for_grouping("qwen3.6-35b-a3b-oq2"),
        "qwen3.6-35b-a3b"
    );
    assert_eq!(
        normalize_model_for_grouping("qwen3.6-35b-a3b-oq4"),
        "qwen3.6-35b-a3b"
    );
    assert_eq!(
        normalize_model_for_grouping("qwen3.6-35b-a3b-ud-oq2"),
        "qwen3.6-35b-a3b"
    );
}

#[test]
fn strips_classic_q_quant_variants() {
    assert_eq!(
        normalize_model_for_grouping("qwen3.6-35b-a3b-q4_k_m"),
        "qwen3.6-35b-a3b"
    );
    assert_eq!(
        normalize_model_for_grouping("qwen3.6-35b-a3b-q8_0"),
        "qwen3.6-35b-a3b"
    );
}

#[test]
fn strips_dot_gguf_with_no_quant_tag() {
    // Bare filename with only a file extension — folds to the base name
    // even though no quant suffix is present.
    assert_eq!(
        normalize_model_for_grouping("my-custom-model.gguf"),
        "my-custom-model"
    );
}

#[test]
fn leaves_unrelated_names_alone() {
    // Note: canonical form is now kebab-case with dash after the family
    // version (`qwen-3.6-*`, not `qwen3.6-*`). `normalize_model_for_grouping`
    // itself is a no-op on these (it only strips GGUF quant suffixs);
    // the dash is introduced by `sql_normalize_model` upstream.
    assert_eq!(
        normalize_model_for_grouping("qwen-3.6-plus"),
        "qwen-3.6-plus"
    );
    assert_eq!(
        normalize_model_for_grouping("qwen-3.6-max-preview"),
        "qwen-3.6-max-preview"
    );
    assert_eq!(
        normalize_model_for_grouping("qwen3.6-35b-a3b"),
        "qwen3.6-35b-a3b"
    );
}

#[test]
fn does_not_strip_dot_gguf_in_middle_of_name() {
    // Defensive: the .gguf strip is suffix-only. A pathological name like
    // `model.gguf-special` must keep the `.gguf-special` part intact so
    // the strip only fires when `.gguf` is the actual filename extension.
    assert_eq!(
        normalize_model_for_grouping("model.gguf-special"),
        "model.gguf-special"
    );
}

// --- sql_normalize_model: provider-namespace prefix handling ---
// 2026-05-17: `dialagram:qwen-3.6-max-preview` and
// `customprovider-qwen:qwen3.6-plus` rendered as their own dashboard
// rows ($8.49 / $2.18) instead of folding under the canonical
// `qwen3.6-max-preview` / `qwen3.6-plus` aggregations. Root cause:
// the SQL+Rust normalizer split on `/` but not on `:`.

#[test]
fn sql_normalize_strips_dialagram_colon_prefix() {
    assert_eq!(
        sql_normalize_model("dialagram:qwen-3.6-max-preview-thinking"),
        "qwen-3.6-max-preview"
    );
    assert_eq!(
        sql_normalize_model("dialagram:qwen-3.6-max-preview"),
        "qwen-3.6-max-preview"
    );
}

#[test]
fn sql_normalize_strips_customprovider_colon_prefix() {
    assert_eq!(
        sql_normalize_model("customprovider-qwen:qwen3.6-plus"),
        "qwen-3.6-plus"
    );
}

#[test]
fn sql_normalize_keeps_slash_handling() {
    assert_eq!(sql_normalize_model("qwen/qwen3.6-plus"), "qwen-3.6-plus");
    assert_eq!(
        sql_normalize_model("opencode/qwen3.6-plus-free"),
        "qwen-3.6-plus"
    );
    // The `/` strip runs first and the `:free` suffix strip kicks in
    // BEFORE the colon-prefix split, so the input reaches the match
    // block as bare `qwen3.6-plus` and normalizes to the kebab canonical.
    assert_eq!(
        sql_normalize_model("qwen/qwen3.6-plus:free"),
        "qwen-3.6-plus"
    );
}

#[test]
fn sql_normalize_strips_thinking_then_colon_prefix() {
    // Order matters: `-thinking` suffix strip must run before the colon
    // split, otherwise `provider:model-thinking` would lose the prefix
    // and still carry the suffix, breaking the canonical-match arms.
    assert_eq!(
        sql_normalize_model("dialagram:qwen-3.6-plus-thinking"),
        "qwen-3.6-plus"
    );
}

#[test]
fn sql_normalize_leaves_canonical_names_alone() {
    assert_eq!(sql_normalize_model("qwen-3.6-plus"), "qwen-3.6-plus");
    assert_eq!(
        sql_normalize_model("qwen-3.6-max-preview"),
        "qwen-3.6-max-preview"
    );
    assert_eq!(sql_normalize_model("opus-4-6"), "opus-4-6");
    assert_eq!(sql_normalize_model("haiku-4-5-20251001"), "haiku-4-5");
}

// --- Qwen 3.7 family consolidation + kebab-case canonicals ---
// 2026-05-30: the /usage dashboard rendered `qwen-3.7-max`,
// `qwen3.7-max`, `qwen-3.7-max-preview`, `qwen3.7-max-20260520`,
// `qwen-latest-series`, and `qwen-latest-series-invite` as six
// separate top-level rows. They are all the same model — collapse to
// one canonical `qwen-3.7-max` bucket. Same for the 3.7 Plus family.
// Also: all canonicals now use kebab-case with a dash after the family
// version (`qwen-3.6-max-preview`, not `qwen3.6-max-preview`), so the
// dashboard has one consistent naming pattern.

#[test]
fn sql_normalize_consolidates_qwen_37_max_family() {
    for variant in [
        "qwen-3.7-max",
        "qwen3.7-max",
        "qwen-3.7-max-preview",
        "qwen3.7-max-preview",
        "qwen-3.7-max-20260520",
        "qwen3.7-max-20260520",
        "qwen-latest-series",
        "qwen-latest-series-invite",
    ] {
        assert_eq!(
            sql_normalize_model(variant),
            "qwen-3.7-max",
            "variant {variant:?} did not collapse to qwen-3.7-max"
        );
    }
}

#[test]
fn sql_normalize_consolidates_qwen_37_plus_family() {
    for variant in [
        "qwen-3.7-plus",
        "qwen3.7-plus",
        "qwen-3.7-plus-preview",
        "qwen3.7-plus-preview",
    ] {
        assert_eq!(
            sql_normalize_model(variant),
            "qwen-3.7-plus",
            "variant {variant:?} did not collapse to qwen-3.7-plus"
        );
    }
}

#[test]
fn sql_normalize_strips_thinking_then_consolidates_37() {
    // `-thinking` strip runs first, then the 3.7 family match.
    assert_eq!(sql_normalize_model("qwen-3.7-max-thinking"), "qwen-3.7-max");
    assert_eq!(
        sql_normalize_model("dialagram:qwen-3.7-max-preview-thinking"),
        "qwen-3.7-max"
    );
    assert_eq!(
        sql_normalize_model("qwen-latest-series-thinking"),
        "qwen-3.7-max"
    );
}

#[test]
fn sql_normalize_normalizes_display_names_with_spaces() {
    // Display-name variants ("Qwen 3.7 Max") must fold into the same
    // kebab-case stream as the CLI names. Lowercasing already happens
    // upstream; this verifies the space → dash swap + family match.
    assert_eq!(sql_normalize_model("Qwen 3.7 Max"), "qwen-3.7-max");
    assert_eq!(
        sql_normalize_model("Qwen 3.6 Max Preview"),
        "qwen-3.6-max-preview"
    );
    assert_eq!(sql_normalize_model("Qwen 3.5 Plus"), "qwen-3.5-plus");
    assert_eq!(sql_normalize_model("Qwen 3.7 Plus"), "qwen-3.7-plus");
}

#[test]
fn sql_normalize_renames_old_no_dash_canonicals_to_kebab() {
    // Legacy canonicals (`qwen3.6-max-preview`, `qwen3.6-plus`,
    // `qwen3.5-plus`) are now accepted as input aliases but the
    // OUTPUT is the kebab form with dash after the family version.
    assert_eq!(
        sql_normalize_model("qwen3.6-max-preview"),
        "qwen-3.6-max-preview"
    );
    assert_eq!(sql_normalize_model("qwen3.6-plus"), "qwen-3.6-plus");
    assert_eq!(sql_normalize_model("qwen3.5-plus"), "qwen-3.5-plus");
}

#[test]
fn sql_normalize_uses_first_colon_for_multi_colon_names() {
    // Hypothetical `provider:family:variant` — strip only the first
    // colon so `family:variant` stays intact rather than collapsing to
    // just `variant`. `rsplit` would over-strip; we use `split_once`.
    assert_eq!(
        sql_normalize_model("provider:family:variant"),
        "family:variant"
    );
}
