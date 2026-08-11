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

use crate::brain::provider::codex_cli::{classify_codex_failure, codex_exit_failure};
use crate::brain::provider::error::ProviderError;

#[test]
fn turn_failed_overload_signals_classify_as_rate_limit() {
    for msg in [
        "Our servers are currently overloaded. Please try again later.",
        "rate limit reached for this account",
        "quota exceeded",
        "HTTP 429: too many requests",
        "server capacity unavailable",
    ] {
        assert!(
            matches!(
                classify_codex_failure(msg, "codex_turn_failed"),
                ProviderError::RateLimitExceeded(_)
            ),
            "expected RateLimitExceeded for: {msg}"
        );
    }
}

#[test]
fn turn_failed_context_length_signals_classify_as_context_length() {
    for msg in [
        "context length exceeded",
        "too many tokens in the request",
        "prompt is too long",
    ] {
        assert!(
            matches!(
                classify_codex_failure(msg, "codex_turn_failed"),
                ProviderError::ContextLengthExceeded(_)
            ),
            "expected ContextLengthExceeded for: {msg}"
        );
    }
}

#[test]
fn turn_failed_generic_maps_to_retryable_500() {
    let err = classify_codex_failure("boom: something exploded", "codex_turn_failed");
    assert!(
        err.is_retryable(),
        "generic turn failure must stay in the retry budget"
    );
    match err {
        ProviderError::ApiError {
            status, error_type, ..
        } => {
            assert_eq!(status, 500);
            assert_eq!(error_type.as_deref(), Some("codex_turn_failed"));
        }
        _ => panic!("expected ApiError for generic turn failure"),
    }
}

#[test]
fn exit_with_stderr_signal_classifies_like_turn_failure() {
    let err = codex_exit_failure(
        "exit status: 1",
        "some banner\nOur servers are currently overloaded. Please try again later.\n",
    );
    assert!(
        matches!(err, ProviderError::RateLimitExceeded(_)),
        "stderr overload signal must classify as RateLimitExceeded"
    );
}

#[test]
fn silent_exit_is_transient_not_internal() {
    // Regression #1004: exit without output used to surface as
    // ProviderError::Internal, which is neither retryable nor
    // fallback-eligible — the turn dropped raw with zero self-healing.
    let err = codex_exit_failure("exit status: 1", "   ");
    assert!(
        matches!(err, ProviderError::StreamError(_)),
        "silent exit must be a transient StreamError"
    );
    assert!(
        err.is_retryable(),
        "silent exit must reach the retry budget and fallback chain"
    );
}
