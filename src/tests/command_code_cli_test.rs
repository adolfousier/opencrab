//! Unit tests for the Command Code CLI provider.
//!
//! These cover the metadata surface (model lists, default model, capability
//! flags) and basic resolver behaviour. We do NOT run a real `command-code -p`
//! here — that requires the user's auth + network and would make CI flaky.

use crate::brain::provider::CommandCodeCliProvider;
use crate::brain::provider::Provider;
use crate::brain::provider::command_code_cli::{DEFAULT_MODEL, SUPPORTED_MODELS};

#[test]
fn default_model_is_deepseek_flash() {
    // Skip on CI: provider construction needs the binary, which isn't on CI.
    let Ok(p) = CommandCodeCliProvider::new() else {
        return;
    };
    // Matches the binary's own default; taste-1 is NOT a real model.
    assert_eq!(p.default_model(), "deepseek/deepseek-v4-flash");
}

#[test]
fn default_model_const_is_a_supported_model() {
    // Guard against a default that `command-code -m <model>` would reject with
    // `unknown model` (the taste-1 regression). No binary needed — pure consts.
    assert!(
        SUPPORTED_MODELS.contains(&DEFAULT_MODEL),
        "DEFAULT_MODEL {DEFAULT_MODEL} must appear in SUPPORTED_MODELS"
    );
    assert!(
        !SUPPORTED_MODELS.contains(&"taste-1"),
        "taste-1 is not real"
    );
}

#[test]
fn with_default_model_overrides() {
    let Ok(p) = CommandCodeCliProvider::new() else {
        return;
    };
    let p = p.with_default_model("claude-sonnet-5".to_string());
    assert_eq!(p.default_model(), "claude-sonnet-5");
}

#[test]
fn supported_models_covers_each_provider_family() {
    let Ok(p) = CommandCodeCliProvider::new() else {
        return;
    };
    let models = p.supported_models();
    // A representative id from each family in `command-code --list-models`.
    assert!(models.iter().any(|m| m == "claude-sonnet-5"));
    assert!(models.iter().any(|m| m == "gpt-5.5"));
    assert!(models.iter().any(|m| m == "google/gemini-3.5-flash"));
    assert!(models.iter().any(|m| m == "deepseek/deepseek-v4-flash"));
    assert!(models.iter().any(|m| m == "zai-org/GLM-5.2"));
    // The phantom flagship must be gone.
    assert!(!models.iter().any(|m| m == "taste-1"));
}

#[test]
fn capability_flags_match_cli_subprocess_pattern() {
    let Ok(p) = CommandCodeCliProvider::new() else {
        return;
    };
    // Mirrors the Claude/Codex/OpenCode CLI surface: command-code runs its own
    // tool loop, so OpenCrabs must NOT re-execute tool_use blocks.
    assert!(p.cli_handles_tools());
    // ...but OpenCrabs DOES own context: we send the full conversation
    // each invocation (`command-code -p` with no session resume).
    assert!(!p.cli_manages_context());
    // Vision goes through analyze_image — `-p` pipe mode has no inline images.
    assert!(!p.supports_vision());
}

#[test]
fn name_is_command_code_cli() {
    let Ok(p) = CommandCodeCliProvider::new() else {
        return;
    };
    assert_eq!(p.name(), "command-code-cli");
}
