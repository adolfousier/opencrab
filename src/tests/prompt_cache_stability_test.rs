//! Prompt-cache stability of the system brain (#657, #658, #681).
//!
//! The cached prefix must stay byte-stable across turns so the provider prompt
//! cache hits. #657 first achieved this by making Runtime Info date-only, but
//! #658 moved the whole Runtime Info block into the UNCACHED suffix (providers
//! stamp cache_control on the stable prefix only). Once the block is uncached, a
//! per-second timestamp no longer touches the cached prefix — so #681 restored
//! the full `Current date & time` line for time-of-day awareness.
//!
//! These tests lock the real invariant: the CACHED PREFIX (what
//! `split_runtime_suffix` leaves behind) is byte-stable even though the suffix
//! carries a second-granular timestamp, and the split boundary keeps per-session
//! lines in the suffix while per-instance constants (Known paths, compiled
//! features) stay cached.

use crate::brain::prompt_builder::{BrainLoader, RuntimeInfo, split_runtime_suffix};
use tempfile::TempDir;

fn loader() -> (TempDir, BrainLoader) {
    let dir = TempDir::new().expect("tempdir");
    let loader = BrainLoader::new(dir.path().to_path_buf());
    (dir, loader)
}

fn runtime_info() -> RuntimeInfo {
    RuntimeInfo {
        model: Some("test-model".to_string()),
        provider: Some("test-provider".to_string()),
        working_directory: Some("~/srv/rs/opencrabs".to_string()),
    }
}

/// A rough `HH:MM:SS` detector — three colon-separated 2-digit runs.
fn contains_seconds_time(s: &str) -> bool {
    s.split_whitespace().any(|tok| {
        let parts: Vec<&str> = tok.split(':').collect();
        parts.len() == 3
            && parts
                .iter()
                .all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_digit()))
    })
}

#[test]
fn runtime_info_now_carries_full_timestamp() {
    // #681: time-of-day restored. The block must render a full date+time line
    // (seconds present) since it rides uncached.
    for brain in [
        loader().1.build_system_brain(Some(&runtime_info())),
        loader().1.build_core_brain(Some(&runtime_info())),
    ] {
        assert!(
            brain.contains("Current date & time:"),
            "expected a full date+time line, got:\n{}",
            brain
                .lines()
                .filter(|l| l.contains("date") || l.contains("Runtime"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        assert!(
            contains_seconds_time(&brain),
            "the restored timestamp must include HH:MM:SS"
        );
    }
}

#[test]
fn cached_prefix_is_byte_stable_despite_second_granular_time() {
    // The real cache guarantee post-#658: the timestamp lives in the UNCACHED
    // suffix, so the cached PREFIX is byte-identical across builds even when the
    // clock ticks between them. (Two full brains may differ by a second; their
    // prefixes must not.)
    let (_dir, loader) = loader();
    let a = loader.build_system_brain(Some(&runtime_info()));
    let b = loader.build_system_brain(Some(&runtime_info()));
    let (prefix_a, suffix_a) = split_runtime_suffix(&a);
    let (prefix_b, _suffix_b) = split_runtime_suffix(&b);
    assert_eq!(
        prefix_a, prefix_b,
        "the cached prefix must be byte-stable across builds (#658)"
    );
    // The volatile timestamp belongs to the suffix, never the prefix.
    assert!(
        !contains_seconds_time(&prefix_a),
        "a per-second timestamp leaked into the CACHED prefix (#658/#681)"
    );
    assert!(
        contains_seconds_time(suffix_a.as_deref().unwrap_or("")),
        "the timestamp must ride in the uncached suffix"
    );
}

#[test]
fn split_boundary_keeps_session_lines_in_suffix_and_constants_cached() {
    // #681 GAP 3: lock the split boundary. Per-SESSION lines (model, provider,
    // working directory, date-time) ride in the uncached suffix; per-INSTANCE
    // constants (Known paths, compiled features) stay in the cached prefix.
    let (_dir, loader) = loader();
    let brain = loader.build_system_brain(Some(&runtime_info()));
    let (prefix, suffix) = split_runtime_suffix(&brain);
    let suffix = suffix.expect("runtime block present");

    for volatile in [
        "Model: test-model",
        "Provider: test-provider",
        "Working directory:",
        "Current date & time:",
    ] {
        assert!(
            suffix.contains(volatile),
            "per-session line must be in the uncached suffix: {volatile:?}"
        );
        assert!(
            !prefix.contains(volatile),
            "per-session line must NOT be in the cached prefix: {volatile:?}"
        );
    }
    // Per-instance constants stay cached.
    assert!(
        prefix.contains("Known paths") && prefix.contains("Built-in features"),
        "Known paths + compiled features must stay in the cached prefix"
    );
}
