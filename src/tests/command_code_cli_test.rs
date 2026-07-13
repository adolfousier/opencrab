//! Unit tests for the Command Code CLI provider.
//!
//! These cover the metadata surface (model lists, default model, capability
//! flags) and basic resolver behaviour. We do NOT run a real `cmd -p` here —
//! that requires the user's auth + network and would make CI flaky.

use crate::brain::provider::CommandCodeCliProvider;
use crate::brain::provider::Provider;

#[test]
fn default_model_is_taste1() {
    // Skip on CI: provider construction needs the binary, which isn't on CI.
    let Ok(p) = CommandCodeCliProvider::new() else {
        return;
    };
    assert_eq!(p.default_model(), "taste-1");
}

#[test]
fn with_default_model_overrides() {
    let Ok(p) = CommandCodeCliProvider::new() else {
        return;
    };
    let p = p.with_default_model("deepseek/deepseek-v4-flash".to_string());
    assert_eq!(p.default_model(), "deepseek/deepseek-v4-flash");
}

#[test]
fn supported_models_includes_recommended_set() {
    let Ok(p) = CommandCodeCliProvider::new() else {
        return;
    };
    let models = p.supported_models();
    // Flagship + a few abridged open-source upstreams from `cmd --list-models`.
    assert!(models.iter().any(|m| m == "taste-1"));
    assert!(models.iter().any(|m| m == "deepseek/deepseek-v4-flash"));
    assert!(models.iter().any(|m| m == "zai-org/GLM-5.2"));
    assert!(models.iter().any(|m| m == "xiaomi/mimo-v2.5"));
}

#[test]
fn capability_flags_match_cli_subprocess_pattern() {
    let Ok(p) = CommandCodeCliProvider::new() else {
        return;
    };
    // Mirrors the Claude/Codex/OpenCode CLI surface: cmd runs its own
    // tool loop, so OpenCrabs must NOT re-execute tool_use blocks.
    assert!(p.cli_handles_tools());
    // ...but OpenCrabs DOES own context: we send the full conversation
    // each invocation (`cmd -p` with no session resume).
    assert!(!p.cli_manages_context());
    // Vision goes through analyze_image because `cmd -p` has no inline image support.
    assert!(!p.supports_vision());
}

#[test]
fn name_is_command_code_cli() {
    let Ok(p) = CommandCodeCliProvider::new() else {
        return;
    };
    assert_eq!(p.name(), "command-code-cli");
}
