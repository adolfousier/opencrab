//! Tests for `normalize_model_for_grouping` — pin the parent-bucket
//! every quantization variant collapses into for the /usage dashboard.
//!
//! Regression anchor: 2026-05-17 screenshot showed
//! `qwen3.6-35b-a3b-ud-iq4_xs.gguf` rendering as its own top-level row
//! ($6.52, 15.5M) instead of folding under `qwen3.6-35b-a3b` alongside
//! `-gguf`, `-oq2`, `-oq4`. Root cause: the model name ended in `.gguf`
//! (file extension with a dot), and the quant patterns only matched
//! `-gguf` / `-ud-iq4_xs` / etc. so nothing stripped.

use crate::usage::data::normalize_model_for_grouping;

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
    assert_eq!(normalize_model_for_grouping("qwen3.6-plus"), "qwen3.6-plus");
    assert_eq!(
        normalize_model_for_grouping("qwen3.6-max-preview"),
        "qwen3.6-max-preview"
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
