//! Tests for `compiled_features` and the "self-awareness" prompt
//! additions that stop the agent from re-implementing capabilities
//! that are already baked into the binary.
//!
//! Trigger case (paraphrased from a real user incident): a newly
//! onboarded user asked the agent to "implement local STT and TTS",
//! and the agent started coding from scratch — local-stt and
//! local-tts are default features with working backends. The agent
//! had no way to know they were compiled in, and no directive
//! telling it to check first.

use crate::brain::prompt_builder::{compiled_features, push_compiled_features};

#[test]
fn telegram_feature_is_compiled_in_test_build() {
    // `cargo test --all-features` is the canonical test command per
    // user policy. Confirms the cfg!() detection actually fires.
    let features = compiled_features();
    assert!(
        features.contains(&"telegram"),
        "telegram should be in compiled features under --all-features; got {features:?}"
    );
}

#[cfg(all(feature = "local-stt", feature = "local-tts"))]
#[test]
fn local_stt_and_tts_are_default_features_in_test_build() {
    // The exact case from the regression — agent shouldn't re-build
    // these when both are default features.
    // Gated behind cfg! because tarpaulin excludes local-stt/local-tts
    // due to ggml linker conflicts (see ci.yml tarpaulin command).
    let features = compiled_features();
    assert!(
        features.contains(&"local-stt"),
        "local-stt should surface so the agent knows STT is built in; got {features:?}"
    );
    assert!(
        features.contains(&"local-tts"),
        "local-tts should surface so the agent knows TTS is built in; got {features:?}"
    );
}

/// Sentinel: if a new `[features]` entry is added to `Cargo.toml` and
/// nobody adds a matching `cfg!(feature = "...")` line in
/// `compiled_features`, this test fails so the omission gets caught
/// in CI rather than weeks later when the agent confidently asks for
/// help implementing a feature that's already there.
///
/// Parses the source file directly for `cfg!(feature = "X")` patterns
/// instead of relying on runtime output, so it works under tarpaulin's
/// limited feature set (which excludes local-stt/local-tts due to
/// ggml linker conflicts).
#[test]
fn all_cargo_features_are_listed() {
    use std::collections::BTreeSet;
    use std::fs;

    // Read Cargo.toml relative to crate root.
    let cargo_toml = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .expect("Cargo.toml must be readable");

    // Pull the [features] block.
    let features_block = cargo_toml
        .split("[features]")
        .nth(1)
        .expect("Cargo.toml must have a [features] section")
        .split("\n[")
        .next()
        .unwrap_or("");

    // Extract feature names: lines like `feature_name = [...]` or
    // `feature_name = []`. Skip `default = ...` since it just lists
    // other features.
    let mut cargo_features: BTreeSet<String> = BTreeSet::new();
    for line in features_block.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if let Some((name, _)) = line.split_once('=') {
            let name = name.trim();
            if name.is_empty() || name == "default" {
                continue;
            }
            cargo_features.insert(name.to_string());
        }
    }

    // Parse the source file for cfg!(feature = "X") patterns in the
    // compiled_features function. This works regardless of which
    // features are actually enabled at test time.
    let source = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/brain/prompt_builder.rs"
    ))
    .expect("prompt_builder.rs must be readable");

    // Find the compiled_features function body.
    let fn_start = source
        .find("fn compiled_features()")
        .expect("compiled_features function must exist");
    let fn_body = &source[fn_start..];
    // Find the closing brace of the function (first `}` after the opening).
    let fn_end = fn_body
        .find("\n}\n")
        .or_else(|| fn_body.find("\n}"))
        .unwrap_or(fn_body.len());
    let fn_body = &fn_body[..fn_end];

    // Extract feature names from cfg!(feature = "X") patterns.
    let mut source_features: BTreeSet<String> = BTreeSet::new();
    for line in fn_body.lines() {
        let trimmed = line.trim();
        if trimmed.contains("cfg!(feature")
            && let Some(start) = trimmed.find('"')
            && let Some(end) = trimmed[start + 1..].find('"')
        {
            let name = &trimmed[start + 1..start + 1 + end];
            source_features.insert(name.to_string());
        }
    }

    let missing: Vec<&String> = cargo_features.difference(&source_features).collect();
    assert!(
        missing.is_empty(),
        "compiled_features() source is missing cfg! branches for Cargo.toml [features]: {missing:?}. \
         Add `if cfg!(feature = \"<name>\") {{ out.push(\"<name>\"); }}` to \
         `compiled_features` in src/brain/prompt_builder.rs."
    );
}

#[test]
fn push_compiled_features_lists_each_active_feature() {
    let mut s = String::new();
    push_compiled_features(&mut s);
    for f in compiled_features() {
        assert!(
            s.contains(f),
            "feature `{f}` is compiled in but missing from prompt output: {s}"
        );
    }
}

#[test]
fn push_compiled_features_tells_agent_to_use_built_ins() {
    let mut s = String::new();
    push_compiled_features(&mut s);
    let lower = s.to_lowercase();
    assert!(
        lower.contains("use the built-in") || lower.contains("already works"),
        "prompt must tell the agent to use built-ins, not re-implement; got: {s}"
    );
    assert!(
        lower.contains("don't re-build") || lower.contains("don't re-implement"),
        "prompt must explicitly forbid re-building; got: {s}"
    );
}

#[test]
fn push_compiled_features_mentions_cargo_features_flag() {
    // When a feature is NOT compiled in, agent should know to ask
    // for a rebuild with --features instead of writing new code.
    let mut s = String::new();
    push_compiled_features(&mut s);
    assert!(
        s.contains("--features"),
        "prompt must mention the cargo --features flag so the agent \
         routes 'feature not enabled' to a rebuild instead of new \
         code; got: {s}"
    );
}

#[test]
fn push_compiled_features_emits_nothing_when_no_features() {
    // Defensive: in a future build with everything disabled, the
    // helper must emit nothing rather than a misleading "Built-in
    // features compiled into this binary: " (with empty list).
    // We can't actually run with no features in this test, but we
    // exercise the code path via the helper's documented contract.
    if compiled_features().is_empty() {
        let mut s = String::new();
        push_compiled_features(&mut s);
        assert!(s.is_empty(), "no features → no output; got: {s}");
    }
}

// ── Self-awareness directive in BRAIN_PREAMBLE ──────────────────

#[test]
fn brain_preamble_has_self_awareness_directive() {
    use crate::brain::prompt_builder::BRAIN_PREAMBLE;
    assert!(
        BRAIN_PREAMBLE.contains("SELF-AWARENESS"),
        "preamble must include the SELF-AWARENESS section header so \
         the agent treats the check-first rule as load-bearing, not \
         buried prose"
    );
}

#[test]
fn brain_preamble_tells_agent_to_check_tool_list_first() {
    use crate::brain::prompt_builder::BRAIN_PREAMBLE;
    let lower = BRAIN_PREAMBLE.to_lowercase();
    assert!(
        lower.contains("check your tool list") || lower.contains("check what you already have"),
        "preamble must explicitly tell the agent to check available \
         tools before proposing new implementations"
    );
}

#[test]
fn brain_preamble_references_compiled_features_line() {
    use crate::brain::prompt_builder::BRAIN_PREAMBLE;
    assert!(
        BRAIN_PREAMBLE.contains("Built-in features"),
        "preamble must reference the Runtime Info line that lists \
         compiled features so the agent knows where to look"
    );
}

#[test]
fn brain_preamble_has_vision_fallback_directive() {
    use crate::brain::prompt_builder::BRAIN_PREAMBLE;
    assert!(
        BRAIN_PREAMBLE.contains("VISION FALLBACK"),
        "preamble must include the VISION FALLBACK section header so the agent \
         self-extracts media when analyze_image/analyze_video isn't exposed by \
         the harness"
    );
}

#[test]
fn brain_preamble_vision_fallback_names_ffmpeg_frame_extraction() {
    use crate::brain::prompt_builder::BRAIN_PREAMBLE;
    let lower = BRAIN_PREAMBLE.to_lowercase();
    // The concrete recovery: read images directly, extract video frames with
    // ffmpeg. Both must be named so the agent doesn't reason abstractly and
    // doesn't fall back to describing media from its filename (hallucination).
    assert!(
        lower.contains("ffmpeg"),
        "vision-fallback directive must name ffmpeg for video frame extraction"
    );
    assert!(
        lower.contains("analyze_image") && lower.contains("analyze_video"),
        "vision-fallback directive must reference both media tools it stands in for"
    );
    assert!(
        lower.contains("filename"),
        "directive must forbid describing media from its filename"
    );
}

#[test]
fn brain_preamble_names_concrete_capabilities_users_might_ask_to_reimplement() {
    use crate::brain::prompt_builder::BRAIN_PREAMBLE;
    // The user incident was specifically STT/TTS. Other common ones:
    // browser automation, messaging channels. List the headline cases
    // so the agent recognizes the pattern without abstract reasoning.
    let lower = BRAIN_PREAMBLE.to_lowercase();
    assert!(
        lower.contains("stt"),
        "STT must be named as a check-first example"
    );
    assert!(
        lower.contains("tts"),
        "TTS must be named as a check-first example"
    );
}
