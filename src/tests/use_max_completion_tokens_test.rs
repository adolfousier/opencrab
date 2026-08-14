//! Tests for `use_max_completion_tokens` config field (#<issue-number>)
//!
//! Verifies that the config override correctly forces `max_completion_tokens`
//! field usage regardless of model name detection. Tests serialize the request
//! to JSON and check which field is present, rather than inspecting private fields.

use crate::brain::provider::custom_openai_compatible::OpenAIProvider;
use crate::brain::provider::types::{LLMRequest, Message};

// ─── Model-based detection (existing behavior) ──────────────────────────

#[test]
fn model_detection_gpt41_uses_completion_tokens() {
    let provider = OpenAIProvider::with_base_url(
        "test-key".to_string(),
        "https://api.example.com/v1".to_string(),
    )
    .with_name("test");

    let request = LLMRequest::new("gpt-4.1-mini", vec![Message::user("hi")]).with_max_tokens(4096);

    let openai_request = provider.to_openai_request(request);
    let json = serde_json::to_value(&openai_request).unwrap();

    // Should have max_completion_tokens, not max_tokens
    assert!(
        json.get("max_completion_tokens").is_some(),
        "gpt-4.1-mini should use max_completion_tokens"
    );
    assert!(
        json.get("max_tokens").is_none() || json["max_tokens"].is_null(),
        "gpt-4.1-mini should not have max_tokens"
    );
}

#[test]
fn model_detection_o_series_uses_completion_tokens() {
    let provider = OpenAIProvider::with_base_url(
        "test-key".to_string(),
        "https://api.example.com/v1".to_string(),
    )
    .with_name("test");

    for model in ["o1-mini", "o1-preview", "o3-mini"] {
        let request = LLMRequest::new(model, vec![Message::user("hi")]).with_max_tokens(4096);

        let openai_request = provider.to_openai_request(request);
        let json = serde_json::to_value(&openai_request).unwrap();

        assert!(
            json.get("max_completion_tokens").is_some(),
            "{} should use max_completion_tokens",
            model
        );
    }
}

#[test]
fn model_detection_older_models_use_max_tokens() {
    let provider = OpenAIProvider::with_base_url(
        "test-key".to_string(),
        "https://api.example.com/v1".to_string(),
    )
    .with_name("test");

    for model in ["gpt-4o", "gpt-4o-mini", "gpt-3.5-turbo"] {
        let request = LLMRequest::new(model, vec![Message::user("hi")]).with_max_tokens(4096);

        let openai_request = provider.to_openai_request(request);
        let json = serde_json::to_value(&openai_request).unwrap();

        assert!(
            json.get("max_tokens").is_some_and(|v| !v.is_null()),
            "{} should use max_tokens",
            model
        );
        assert!(
            json.get("max_completion_tokens").is_none() || json["max_completion_tokens"].is_null(),
            "{} should not have max_completion_tokens",
            model
        );
    }
}

// ─── Config override behavior ──────────────────────────────────────────

#[test]
fn config_override_true_forces_completion_tokens_regardless_of_model() {
    // Even for models that normally use max_tokens, the override should
    // force max_completion_tokens
    let provider = OpenAIProvider::with_base_url(
        "test-key".to_string(),
        "https://api.example.com/v1".to_string(),
    )
    .with_name("test")
    .with_use_max_completion_tokens(true);

    let request = LLMRequest::new("gpt-4o", vec![Message::user("hi")]).with_max_tokens(4096);

    let openai_request = provider.to_openai_request(request);
    let json = serde_json::to_value(&openai_request).unwrap();

    // Should have max_completion_tokens set, max_tokens should be null/absent
    assert!(
        json.get("max_completion_tokens")
            .is_some_and(|v| v.as_u64() == Some(4096)),
        "Config override=true should force max_completion_tokens for gpt-4o"
    );
    assert!(
        json.get("max_tokens").is_none() || json["max_tokens"].is_null(),
        "Config override=true should not have max_tokens"
    );
}

#[test]
fn config_override_false_forces_max_tokens_for_newer_models() {
    // Even for models that normally use max_completion_tokens, setting
    // the override to false should force max_tokens
    let provider = OpenAIProvider::with_base_url(
        "test-key".to_string(),
        "https://api.example.com/v1".to_string(),
    )
    .with_name("test")
    .with_use_max_completion_tokens(false);

    let request = LLMRequest::new("gpt-4.1", vec![Message::user("hi")]).with_max_tokens(8192);

    let openai_request = provider.to_openai_request(request);
    let json = serde_json::to_value(&openai_request).unwrap();

    // Should have max_tokens set, max_completion_tokens should be null/absent
    assert!(
        json.get("max_tokens")
            .is_some_and(|v| v.as_u64() == Some(8192)),
        "Config override=false should force max_tokens for gpt-4.1"
    );
    assert!(
        json.get("max_completion_tokens").is_none() || json["max_completion_tokens"].is_null(),
        "Config override=false should not have max_completion_tokens"
    );
}

#[test]
fn no_config_override_uses_model_detection() {
    // Without the override, behavior falls back to model detection
    let provider = OpenAIProvider::with_base_url(
        "test-key".to_string(),
        "https://api.example.com/v1".to_string(),
    )
    .with_name("test");
    // No with_use_max_completion_tokens call - uses default (None)

    // gpt-4o should use max_tokens
    let request_old = LLMRequest::new("gpt-4o", vec![Message::user("hi")]).with_max_tokens(4096);
    let openai_request_old = provider.to_openai_request(request_old);
    let json_old = serde_json::to_value(&openai_request_old).unwrap();

    assert!(
        json_old.get("max_tokens").is_some_and(|v| !v.is_null()),
        "gpt-4o without override should use max_tokens"
    );

    // gpt-4.1 should use max_completion_tokens
    let request_new = LLMRequest::new("gpt-4.1", vec![Message::user("hi")]).with_max_tokens(4096);
    let openai_request_new = provider.to_openai_request(request_new);
    let json_new = serde_json::to_value(&openai_request_new).unwrap();

    assert!(
        json_new
            .get("max_completion_tokens")
            .is_some_and(|v| !v.is_null()),
        "gpt-4.1 without override should use max_completion_tokens"
    );
}

#[test]
fn scaleway_example_config_works() {
    // Realistic test: Scaleway-like provider with override enabled
    let provider = OpenAIProvider::with_base_url(
        "scaleway-key".to_string(),
        "https://api.scaleway.ai/v1/chat/completions".to_string(),
    )
    .with_name("scaleway")
    .with_use_max_completion_tokens(true);

    // Test with a model that would normally use max_tokens
    let request = LLMRequest::new("mistral-large", vec![Message::user("hi")]).with_max_tokens(2048);

    let openai_request = provider.to_openai_request(request);
    let json = serde_json::to_value(&openai_request).unwrap();

    // Should use max_completion_tokens due to config override
    assert!(
        json.get("max_completion_tokens")
            .is_some_and(|v| v.as_u64() == Some(2048)),
        "Scaleway config should force max_completion_tokens for all models"
    );
}
