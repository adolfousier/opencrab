use super::*;

#[test]
fn test_anthropic_provider_creation() {
    let provider = AnthropicProvider::new("test-key".to_string());
    assert_eq!(provider.name(), "anthropic");
    assert_eq!(provider.default_model(), "claude-opus-4-8");
}

#[test]
fn test_custom_default_model() {
    let provider = AnthropicProvider::new("test-key".to_string())
        .with_default_model("claude-opus-4-6".to_string());
    assert_eq!(provider.default_model(), "claude-opus-4-6");
}

#[test]
fn test_supported_models() {
    let provider = AnthropicProvider::new("test-key".to_string());
    let models = provider.supported_models();
    assert!(models.contains(&"claude-opus-4-6".to_string()));
    assert!(models.contains(&"claude-sonnet-4-5-20250929".to_string()));
    assert!(models.contains(&"claude-haiku-4-5-20251001".to_string()));
    // Legacy models still present
    assert!(models.contains(&"claude-3-opus-20240229".to_string()));
}

#[test]
fn test_context_window() {
    let provider = AnthropicProvider::new("test-key".to_string());
    assert_eq!(provider.context_window("claude-opus-4-6"), Some(1_000_000));
    assert_eq!(
        provider.context_window("claude-3-opus-20240229"),
        Some(200_000)
    );
    assert_eq!(provider.context_window("unknown-model"), None);
}

#[test]
fn test_cost_calculation() {
    let provider = AnthropicProvider::new("test-key".to_string());

    // Test Opus 4 pricing (corrected: $5/$25 per OpenRouter 2026-02-25)
    let cost = provider.calculate_cost("claude-opus-4-6", 1_000_000, 1_000_000);
    assert_eq!(cost, 30.0); // $5 input + $25 output

    // Test Sonnet 4.6 pricing (was missing — main model)
    let cost = provider.calculate_cost("claude-sonnet-4-6", 1_000_000, 1_000_000);
    assert_eq!(cost, 18.0); // $3 input + $15 output

    // Test legacy Opus 3 pricing ($15/$75)
    let cost = provider.calculate_cost("claude-3-opus-20240229", 1_000_000, 1_000_000);
    assert_eq!(cost, 90.0);

    // Test Haiku 4.5 pricing ($1/$5)
    let cost = provider.calculate_cost("claude-haiku-4-5-20251001", 1_000_000, 1_000_000);
    assert_eq!(cost, 6.0); // $1 input + $5 output

    // Test legacy Haiku 3.5 pricing
    let cost = provider.calculate_cost("claude-3-5-haiku-20241022", 1_000_000, 1_000_000);
    assert_eq!(cost, 4.8); // $0.80 input + $4.0 output

    // Test legacy Haiku pricing
    let cost = provider.calculate_cost("claude-3-haiku-20240307", 1_000_000, 1_000_000);
    assert_eq!(cost, 1.5); // $0.25 input + $1.25 output
}

#[test]
fn test_standard_headers() {
    let provider = AnthropicProvider::new("sk-ant-api-key".to_string());
    let headers = provider.headers();
    assert!(headers.contains_key("x-api-key"));
    assert!(!headers.contains_key(reqwest::header::AUTHORIZATION));
    assert!(headers.contains_key("anthropic-beta"));
}

#[test]
fn test_capabilities() {
    let provider = AnthropicProvider::new("test-key".to_string());
    assert!(provider.supports_streaming());
    assert!(provider.supports_tools());
    assert!(provider.supports_vision());
}
