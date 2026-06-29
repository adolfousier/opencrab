use super::*;
use crate::brain::provider::LLMRequest;
use crate::brain::provider::LLMResponse;
use crate::brain::provider::ProviderCapabilities;
use crate::brain::provider::ProviderStream;
use async_trait::async_trait;

/// Mock provider for testing
struct MockProvider;

#[async_trait]
impl Provider for MockProvider {
    async fn complete(&self, _request: LLMRequest) -> Result<LLMResponse> {
        unimplemented!("Mock provider")
    }

    async fn stream(&self, _request: LLMRequest) -> Result<ProviderStream> {
        unimplemented!("Mock provider")
    }

    fn name(&self) -> &str {
        "mock"
    }

    fn default_model(&self) -> &str {
        "mock-model-1"
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["mock-model-1".to_string(), "mock-model-2".to_string()]
    }

    fn context_window(&self, _model: &str) -> Option<u32> {
        Some(4096)
    }

    fn calculate_cost(&self, _model: &str, _input: u32, _output: u32) -> f64 {
        0.0
    }
}

#[test]
fn test_provider_validate_model() {
    let provider = MockProvider;
    assert!(provider.validate_model("mock-model-1"));
    assert!(provider.validate_model("mock-model-2"));
    assert!(!provider.validate_model("unknown-model"));
}

#[test]
fn test_provider_capabilities() {
    let provider = MockProvider;
    let caps = ProviderCapabilities::for_provider(&provider);
    assert!(caps.streaming);
    assert!(caps.tools);
    assert!(!caps.vision);
}
