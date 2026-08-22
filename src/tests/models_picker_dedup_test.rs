//! #1149 — the /models picker stopped duplicating what its inline keyboard
//! already renders, and the ✓/•/🔒 semantics + the "apply to all sessions"
//! payload gained single sources.
//!
//! Pre-fix: `format_providers` enumerated every provider in the text body
//! while each channel drew one button per provider right below (identical to
//! the #129 /sessions duplication); all three model-list paths printed a
//! numbered text copy of the same models the buttons carry; and the marker
//! logic existed as three hand-rolled copies across telegram/discord/slack.

use crate::channels::commands::{apply_all_callback_data, models_for_provider, provider_marker};
use crate::config::profile::{home_for_profile, with_profile_home_async};

// ── provider_marker: one source for ✓/•/🔒 (#1149) ──────────────────────────

#[test]
fn unconfigured_provider_is_locked_regardless_of_current() {
    assert_eq!(provider_marker("openai", "openai", false), "🔒");
    assert_eq!(provider_marker("openai", "mock", false), "🔒");
}

#[test]
fn configured_current_provider_gets_a_checkmark() {
    assert_eq!(provider_marker("mock", "mock", true), "✓");
}

#[test]
fn configured_other_provider_gets_a_bullet() {
    assert_eq!(provider_marker("openai", "mock", true), "•");
}

// ── apply_all_callback_data: central 64-byte guard (#468 → #1149) ──────────

#[test]
fn short_pair_builds_the_literal_allm_payload() {
    let data = apply_all_callback_data("openrouter", "stealth/ox-alpha").expect("fits");
    assert_eq!(data, "allm:openrouter|stealth/ox-alpha");
    assert!(data.len() <= 64);
}

#[test]
fn custom_prefixes_and_free_suffixes_survive_the_pipe_format() {
    let data = apply_all_callback_data("custom:modelscope", "Qwen/Qwen3-235B:free").expect("fits");
    assert_eq!(data, "allm:custom:modelscope|Qwen/Qwen3-235B:free");
}

#[test]
fn overflowing_pair_returns_none_instead_of_a_broken_button() {
    // 65+ bytes is Telegram's hard callback_data rejection; no index-fallback
    // form exists for allm:, so overflow means NO button.
    let long_model = "x".repeat(70);
    assert!(apply_all_callback_data("custom:p", &long_model).is_none());
}

// ── models_for_provider body: heading + current + hint, no enumeration ─────

/// Write config.toml + empty keys.toml under the given home path.
fn write_profile_home(home: &std::path::Path, config_toml: &str) {
    std::fs::create_dir_all(home).expect("create profile home");
    std::fs::write(home.join("config.toml"), config_toml).expect("write config");
    std::fs::write(home.join("keys.toml"), b"").expect("write keys");
}

#[tokio::test]
async fn custom_provider_model_body_has_no_numbered_enumeration() {
    let profile = format!("test_picker_dedup_{}", uuid::Uuid::new_v4());
    let home = home_for_profile(Some(&profile));
    write_profile_home(
        &home,
        r#"
[providers.custom.qwen-mlx]
enabled = true
base_url = "http://localhost:8080/v1"
api_key = "test-key"
default_model = "Qwen-Ambassador/Qwen3.7-Max"
models = ["Qwen-Ambassador/Qwen3.7-Max", "stale-placeholder"]
"#,
    );

    with_profile_home_async(Some(&profile), async {
        let resp = models_for_provider("custom:qwen-mlx").await;

        assert!(!resp.models.is_empty(), "buttons must still be offered");
        // Body carries the current model…
        assert!(
            resp.text.contains("Current: `Qwen-Ambassador/Qwen3.7-Max`"),
            "got: {}",
            resp.text
        );
        // …and the hint, but NOT a numbered copy of the button list.
        assert!(
            resp.text.contains("Tap a model below"),
            "got: {}",
            resp.text
        );
        assert!(
            !resp.text.contains("1. `"),
            "numbered enumeration must be gone (#1149), got: {}",
            resp.text
        );
        assert!(
            !resp.text.contains("stale-placeholder"),
            "model names belong in buttons only (#1149), got: {}",
            resp.text
        );
    })
    .await;
}
