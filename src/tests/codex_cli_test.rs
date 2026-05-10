//! Unit tests for the Codex CLI provider.
//!
//! These cover the metadata surface (model lists, default model, capability
//! flags) and basic resolver behaviour. We do NOT run a real `codex exec`
//! here — that requires the user's auth + network and would make CI flaky.

use crate::brain::provider::CodexCliProvider;
use crate::brain::provider::Provider;

#[test]
fn default_model_is_gpt55() {
    // Skip on CI: provider construction needs the binary, which isn't on CI.
    let Ok(p) = CodexCliProvider::new() else {
        return;
    };
    assert_eq!(p.default_model(), "gpt-5.5");
}

#[test]
fn with_default_model_overrides() {
    let Ok(p) = CodexCliProvider::new() else {
        return;
    };
    let p = p.with_default_model("gpt-5.3-codex".to_string());
    assert_eq!(p.default_model(), "gpt-5.3-codex");
}

#[test]
fn supported_models_includes_recommended_set() {
    let Ok(p) = CodexCliProvider::new() else {
        return;
    };
    let models = p.supported_models();
    // Recommended (per developers.openai.com/codex/models)
    assert!(models.iter().any(|m| m == "gpt-5.5"));
    assert!(models.iter().any(|m| m == "gpt-5.4"));
    assert!(models.iter().any(|m| m == "gpt-5.4-mini"));
    assert!(models.iter().any(|m| m == "gpt-5.3-codex"));
}

#[test]
fn capability_flags_match_cli_subprocess_pattern() {
    let Ok(p) = CodexCliProvider::new() else {
        return;
    };
    // Mirrors the Claude CLI / OpenCode CLI surface: codex runs its own
    // tool loop, so OpenCrabs must NOT re-execute tool_use blocks.
    assert!(p.cli_handles_tools());
    // ...but OpenCrabs DOES own context: we send the full conversation
    // each invocation (`--ephemeral`, no `--resume`).
    assert!(!p.cli_manages_context());
    // Vision goes through analyze_image because we don't pass `-i <FILE>`.
    assert!(!p.supports_vision());
}

#[test]
fn name_is_codex_cli() {
    let Ok(p) = CodexCliProvider::new() else {
        return;
    };
    assert_eq!(p.name(), "codex-cli");
}
