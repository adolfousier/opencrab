//! Pin the canonical CLI provider model lists so the `/models` menu in
//! channel handlers can never drift from what the provider actually serves.
//!
//! The 2026-05-27 bug ("OpenCode CLI menu shows Claude model names") happened
//! because `channels::commands::models_for_provider` hardcoded a duplicate
//! list that diverged from `OpenCodeCliProvider::supported_models()`. Now
//! both read from `pub(crate) const SUPPORTED_MODELS` in the provider
//! module, surfaced via `utils::providers::cli_supported_models`. These
//! tests fail loudly if anyone reintroduces a parallel list.

use crate::brain::provider::{
    Provider, claude_cli::ClaudeCliProvider, opencode_cli::OpenCodeCliProvider,
};
use crate::utils::providers::cli_supported_models;

#[test]
fn claude_cli_menu_matches_provider_supported_models() {
    // We don't construct ClaudeCliProvider here because `new()` calls
    // resolve_claude_path which probes the filesystem. Instead we read the
    // const directly through the helper and compare to the static
    // `SUPPORTED_MODELS` list the provider's trait impl uses.
    let (menu_models, _) =
        cli_supported_models("claude-cli").expect("claude-cli must be a known CLI provider");
    let provider_models: Vec<String> = crate::brain::provider::claude_cli::SUPPORTED_MODELS
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        menu_models, provider_models,
        "menu and provider must read from the same SUPPORTED_MODELS const"
    );
}

#[test]
fn opencode_cli_menu_matches_provider_supported_models() {
    let (menu_models, _) =
        cli_supported_models("opencode-cli").expect("opencode-cli must be a known CLI provider");
    let provider_models: Vec<String> = crate::brain::provider::opencode_cli::SUPPORTED_MODELS
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        menu_models, provider_models,
        "menu and provider must read from the same SUPPORTED_MODELS const"
    );
}

#[test]
fn command_code_cli_menu_matches_provider_supported_models() {
    let (menu_models, default) = cli_supported_models("command-code-cli")
        .expect("command-code-cli must be a known CLI provider");
    let provider_models: Vec<String> = crate::brain::provider::command_code_cli::SUPPORTED_MODELS
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        menu_models, provider_models,
        "menu and provider must read from the same SUPPORTED_MODELS const"
    );
    // The menu default must be a model the CLI actually accepts (taste-1 was a
    // phantom that failed `command-code -m taste-1` with `unknown model`).
    assert!(
        menu_models.iter().any(|m| m == default),
        "default {default} must appear in the menu list"
    );
    assert!(
        !menu_models.iter().any(|m| m == "taste-1"),
        "taste-1 is not a real Command Code model"
    );
}

#[test]
fn underscore_alias_resolves_same_list() {
    let (hyphen, _) = cli_supported_models("opencode-cli").unwrap();
    let (underscore, _) = cli_supported_models("opencode_cli").unwrap();
    assert_eq!(
        hyphen, underscore,
        "hyphen and underscore aliases must resolve to the same canonical list"
    );

    let (hyphen_c, _) = cli_supported_models("claude-cli").unwrap();
    let (underscore_c, _) = cli_supported_models("claude_cli").unwrap();
    assert_eq!(hyphen_c, underscore_c);

    // Command Code exposes a third alias (`cmd-cli`) alongside the hyphen and
    // underscore forms — all three must resolve to the same canonical list.
    let (hyphen_cc, _) = cli_supported_models("command-code-cli").unwrap();
    let (underscore_cc, _) = cli_supported_models("command_code_cli").unwrap();
    let (short_cc, _) = cli_supported_models("cmd-cli").unwrap();
    assert_eq!(hyphen_cc, underscore_cc);
    assert_eq!(hyphen_cc, short_cc);
}

#[test]
fn unknown_provider_returns_none() {
    assert!(cli_supported_models("openai").is_none());
    assert!(cli_supported_models("qwen").is_none());
    assert!(cli_supported_models("anthropic").is_none());
    assert!(cli_supported_models("").is_none());
}

#[test]
fn opencode_cli_does_not_list_claude_names() {
    // Pin the specific 2026-05-27 regression: OpenCode CLI menu must NOT
    // contain Claude model names like sonnet-4.5 or opus-4.1.
    let (models, default) = cli_supported_models("opencode-cli").unwrap();
    for m in &models {
        assert!(
            !m.contains("sonnet") && !m.contains("opus") && !m.contains("haiku"),
            "OpenCode CLI must not list Claude model '{m}' — regression of 2026-05-27 bug"
        );
    }
    assert!(
        default.starts_with("opencode/"),
        "OpenCode CLI default must be an opencode/* model, got: {default}"
    );
}

#[test]
fn claude_cli_default_is_real_claude_model() {
    let (_, default) = cli_supported_models("claude-cli").unwrap();
    assert!(
        default.starts_with("opus")
            || default.starts_with("sonnet")
            || default.starts_with("haiku"),
        "Claude CLI default must be a real Claude model, got: {default}"
    );
}

// Sanity-check the provider's trait method actually returns the const,
// not some other list. Touch every variant the provider could synthesize.

#[test]
fn claude_cli_trait_supported_models_uses_const() {
    if let Ok(p) = ClaudeCliProvider::new() {
        let trait_models = p.supported_models();
        let const_models: Vec<String> = crate::brain::provider::claude_cli::SUPPORTED_MODELS
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(trait_models, const_models);
    }
    // If `claude` binary isn't installed in this test env, skip silently;
    // the menu helper test above already pins the const.
}

#[test]
fn opencode_cli_trait_supported_models_uses_const() {
    if let Ok(p) = OpenCodeCliProvider::new() {
        let trait_models = p.supported_models();
        let const_models: Vec<String> = crate::brain::provider::opencode_cli::SUPPORTED_MODELS
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(trait_models, const_models);
    }
}
