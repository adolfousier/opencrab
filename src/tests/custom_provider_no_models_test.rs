//! Pin the "custom provider with no configured models returns empty
//! models + help text" behaviour added 2026-05-28.
//!
//! Pre-fix: a custom provider with neither `default_model` nor a
//! populated `models` list rendered a single inline-keyboard button
//! labeled "unknown (no models configured)". Clicking it called
//! `set_session_model` with that literal string and the agent silently
//! broke. User report: Telegram model switch for `custom:qwen-mlx`
//! (freshly merge-created from keys.toml) appeared to do nothing.
//!
//! Post-fix: empty `models` list + help text body. The channel handler
//! shows the help text instead of rendering an inert button.
//!
//! `models_for_provider` is tightly coupled to `Config::load()` so we
//! exercise the contract via a task-local profile-home override
//! (`with_profile_home_async`) pointed at a throwaway profile — no
//! process-wide `HOME` mutation, so no process-wide env lock is needed.

use crate::channels::commands::models_for_provider;
use crate::config::profile::{home_for_profile, with_profile_home_async};

/// Write config.toml + empty keys.toml under the given home path.
/// Returns the home path (not a TempDir — profile directories live under
/// ~/.opencrabs/profiles/ and persist for test isolation).
fn write_profile_home(home: &std::path::Path, config_toml: &str) {
    std::fs::create_dir_all(home).expect("create profile home");
    std::fs::write(home.join("config.toml"), config_toml).expect("write config");
    // Empty keys.toml — config we test sets api_key inline.
    std::fs::write(home.join("keys.toml"), b"").expect("write keys");
}

#[tokio::test]
async fn empty_custom_provider_returns_empty_models_and_help_text() {
    let profile = format!("test_no_models_{}", uuid::Uuid::new_v4());
    let home = home_for_profile(Some(&profile));
    write_profile_home(
        &home,
        r#"
[providers.custom.qwen-mlx]
enabled = true
base_url = "http://localhost:8080/v1"
api_key = "test-key"
# Deliberately no default_model and no models list
"#,
    );

    with_profile_home_async(Some(&profile), async {
        let resp = models_for_provider("custom:qwen-mlx").await;

        assert!(
            resp.models.is_empty(),
            "custom provider with no default_model + empty models list must return \
             empty models, NOT a placeholder button labeled 'unknown (no models \
             configured)'. Got models: {:?}",
            resp.models
        );
        assert!(
            resp.text.contains("No models configured"),
            "must show 'No models configured' help text, got: {}",
            resp.text
        );
        assert!(
            resp.text.contains("default_model"),
            "help text must mention default_model so the user knows what to add"
        );
        assert!(
            resp.text.contains("[providers.custom.qwen-mlx]"),
            "help text must include the TOML section for the specific provider, got: {}",
            resp.text
        );
    })
    .await;
}

#[tokio::test]
async fn custom_provider_with_default_model_returns_real_button() {
    let profile = format!("test_with_default_{}", uuid::Uuid::new_v4());
    let home = home_for_profile(Some(&profile));
    write_profile_home(
        &home,
        r#"
[providers.custom.qwen-mlx]
enabled = true
base_url = "http://localhost:8080/v1"
api_key = "test-key"
default_model = "qwen3-7b-mlx-4bit"
"#,
    );

    with_profile_home_async(Some(&profile), async {
        let resp = models_for_provider("custom:qwen-mlx").await;

        assert!(
            !resp.models.is_empty(),
            "custom provider WITH default_model must produce a real model list"
        );
        assert!(
            resp.models.contains(&"qwen3-7b-mlx-4bit".to_string()),
            "real default_model must appear in the picker, got: {:?}",
            resp.models
        );
        assert!(
            !resp.text.contains("No models configured"),
            "must NOT show the empty-config help text when default_model is set"
        );
        assert!(
            !resp.text.contains("unknown (no models configured)"),
            "must NEVER include the pre-fix placeholder string"
        );
    })
    .await;
}

/// #267: the configured `default_model` is the authoritative current model
/// and must be surfaced on top, marked selected — even when the stored
/// `models` list starts with a stale placeholder that is not the default.
#[tokio::test]
async fn default_model_shown_on_top_over_stale_models_list() {
    let profile = format!("test_default_top_{}", uuid::Uuid::new_v4());
    let home = home_for_profile(Some(&profile));
    write_profile_home(
        &home,
        r#"
[providers.custom.modelscope]
enabled = true
base_url = "https://api-inference.modelscope.ai/v1"
api_key = "test-key"
default_model = "Qwen-Ambassador/Qwen3.7-Max"
models = ["kimi-k2.5", "glm-5", "MiniMax-M2.7"]
"#,
    );

    with_profile_home_async(Some(&profile), async {
        let resp = models_for_provider("custom:modelscope").await;

        assert_eq!(
            resp.current_model, "Qwen-Ambassador/Qwen3.7-Max",
            "current must be the configured default_model, not the first stored (stale) entry"
        );
        assert_eq!(
            resp.models.first().map(String::as_str),
            Some("Qwen-Ambassador/Qwen3.7-Max"),
            "the default_model must be listed on top, got: {:?}",
            resp.models
        );
        // The stale entries are still offered (the list is not discarded), but the
        // default is no longer buried or missing.
        assert!(
            resp.models.contains(&"kimi-k2.5".to_string()),
            "stored models remain available, got: {:?}",
            resp.models
        );
        assert!(
            resp.text.contains("Current: `Qwen-Ambassador/Qwen3.7-Max`"),
            "header must show the default as current, got: {}",
            resp.text
        );
    })
    .await;
}
