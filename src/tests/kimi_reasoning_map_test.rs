//! Tests for Kimi reasoning-control mapping, validated per model (#613).

use crate::brain::provider::kimi_reasoning::{
    ReasoningError, ReasoningPatch, patch_fields, resolve, resolve_fields, streams_reasoning_inline,
};

#[test]
fn coding_endpoint_streams_reasoning_inline() {
    // Kimi Code endpoint inlines reasoning as content — must be detected.
    assert!(streams_reasoning_inline(Some(
        "https://api.kimi.com/coding/v1/chat/completions"
    )));
    // Moonshot API endpoint separates reasoning — must NOT match.
    assert!(!streams_reasoning_inline(Some(
        "https://api.moonshot.ai/v1/chat/completions"
    )));
    // Other providers and hardcoded-endpoint providers never match.
    assert!(!streams_reasoning_inline(Some(
        "https://openrouter.ai/api/v1/chat/completions"
    )));
    assert!(!streams_reasoning_inline(None));
}

#[test]
fn k3_accepts_only_max() {
    assert_eq!(
        resolve("kimi-k3", "max"),
        Ok(Some(ReasoningPatch::Effort("max".to_string())))
    );
    // K3 rejects low/high today — Kimi has not shipped them.
    assert_eq!(
        resolve("kimi-k3", "high"),
        Err(ReasoningError {
            family: "kimi-k3",
            allowed: "max",
        })
    );
    assert!(resolve("k3", "off").is_err());
}

#[test]
fn k27_code_is_always_on_and_cannot_disable() {
    assert_eq!(
        resolve("kimi-for-coding", "on"),
        Ok(Some(ReasoningPatch::Thinking(true)))
    );
    assert_eq!(
        resolve("kimi-k2.7-code", "enabled"),
        Ok(Some(ReasoningPatch::Thinking(true)))
    );
    // Disabling is rejected — k2.7-code thinking is always active.
    assert!(resolve("kimi-for-coding", "off").is_err());
}

#[test]
fn k26_supports_real_toggle() {
    assert_eq!(
        resolve("kimi-k2.6", "on"),
        Ok(Some(ReasoningPatch::Thinking(true)))
    );
    assert_eq!(
        resolve("kimi-k2.6", "off"),
        Ok(Some(ReasoningPatch::Thinking(false)))
    );
    assert!(resolve("kimi-k2.6", "gibberish").is_err());
}

#[test]
fn non_kimi_model_is_a_noop() {
    assert_eq!(resolve("gpt-5", "max"), Ok(None));
    assert_eq!(resolve("claude-opus-4-8", "on"), Ok(None));
    assert_eq!(resolve_fields("gpt-5", "max"), (None, None));
}

#[test]
fn patch_fields_split_correctly() {
    assert_eq!(
        patch_fields(&ReasoningPatch::Effort("max".to_string())),
        (Some("max".to_string()), None)
    );
    let (effort, thinking) = patch_fields(&ReasoningPatch::Thinking(true));
    assert_eq!(effort, None);
    assert_eq!(thinking, Some(serde_json::json!({ "type": "enabled" })));
    let (_, thinking_off) = patch_fields(&ReasoningPatch::Thinking(false));
    assert_eq!(
        thinking_off,
        Some(serde_json::json!({ "type": "disabled" }))
    );
}

#[test]
fn resolve_fields_skips_invalid_value_silently() {
    // Config-default path: an invalid value for the model must be a no-op,
    // never a partial/garbage body field.
    assert_eq!(resolve_fields("kimi-k3", "low"), (None, None));
    assert_eq!(
        resolve_fields("kimi-k3", "max"),
        (Some("max".to_string()), None)
    );
}
