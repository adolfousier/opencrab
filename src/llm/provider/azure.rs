//! Azure OpenAI Provider
//!
//! Wrapper around OpenAI provider for Azure OpenAI Service.
//! Azure OpenAI uses the same API format as OpenAI but with different endpoints.

use super::{openai::OpenAIProvider, LLMRequest, LLMResponse, Provider, Result};
use async_trait::async_trait;

/// Azure OpenAI Provider
///
/// Uses the OpenAI-compatible API but configured for Azure endpoints.
/// Azure endpoint format: https://{resource-name}.openai.azure.com/openai/deployments/{deployment-id}/chat/completions?api-version=2024-02-15-preview
pub struct AzureOpenAIProvider {
    inner: OpenAIProvider,
    #[allow(dead_code)]
    resource_name: String,
    #[allow(dead_code)]
    deployment_id: String,
}

impl AzureOpenAIProvider {
    /// Create a new Azure OpenAI provider
    ///
    /// # Arguments
    /// * `api_key` - Azure OpenAI API key
    /// * `resource_name` - Azure OpenAI resource name (from Azure portal)
    /// * `deployment_id` - Deployment ID/model deployment name
    ///
    /// # Example
    /// ```
    /// use opencrabs::llm::provider::azure::AzureOpenAIProvider;
    ///
    /// let provider = AzureOpenAIProvider::new(
    ///     "your-api-key".to_string(),
    ///     "my-resource".to_string(),
    ///     "gpt-4-deployment".to_string(),
    /// );
    /// ```
    pub fn new(api_key: String, resource_name: String, deployment_id: String) -> Self {
        let base_url = format!(
            "https://{}.openai.azure.com/openai/deployments/{}/chat/completions?api-version=2024-02-15-preview",
            resource_name, deployment_id
        );

        let inner = OpenAIProvider::with_base_url(api_key, base_url);

        Self {
            inner,
            resource_name,
            deployment_id,
        }
    }

    /// Set a custom default model
    pub fn with_default_model(mut self, model: String) -> Self {
        self.inner = self.inner.with_default_model(model);
        self
    }
}

#[async_trait]
impl Provider for AzureOpenAIProvider {
    fn name(&self) -> &str {
        "azure-openai"
    }

    fn default_model(&self) -> &str {
        "gpt-35-turbo"
    }

    async fn complete(&self, request: LLMRequest) -> Result<LLMResponse> {
        self.inner.complete(request).await
    }

    async fn stream(&self, request: LLMRequest) -> Result<super::ProviderStream> {
        self.inner.stream(request).await
    }

    fn supported_models(&self) -> Vec<String> {
        // Azure uses deployment IDs, not model names
        vec![
            "gpt-4".to_string(),
            "gpt-4-32k".to_string(),
            "gpt-35-turbo".to_string(),
            "gpt-35-turbo-16k".to_string(),
        ]
    }

    fn context_window(&self, model: &str) -> Option<u32> {
        Some(match model {
            "gpt-4-32k" => 32768,
            "gpt-35-turbo-16k" => 16384,
            _ => 8192, // Default for gpt-4 and gpt-35-turbo
        })
    }

    fn calculate_cost(&self, model: &str, input_tokens: u32, output_tokens: u32) -> f64 {
        // Azure OpenAI pricing (as of 2024)
        // Prices may vary by region
        let (input_price, output_price) = match model {
            "gpt-4" => (0.03, 0.06), // per 1K tokens
            "gpt-4-32k" => (0.06, 0.12),
            "gpt-35-turbo" => (0.0015, 0.002),
            "gpt-35-turbo-16k" => (0.003, 0.004),
            _ => (0.03, 0.06), // Default to GPT-4 pricing
        };

        let input_cost = (input_tokens as f64 / 1000.0) * input_price;
        let output_cost = (output_tokens as f64 / 1000.0) * output_price;

        input_cost + output_cost
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_azure_provider_creation() {
        let provider = AzureOpenAIProvider::new(
            "test-key".to_string(),
            "my-resource".to_string(),
            "gpt-4".to_string(),
        );

        assert_eq!(provider.name(), "azure-openai");
        assert!(provider.supports_streaming());
        assert!(provider.supports_tools());
    }

    #[test]
    fn test_azure_context_window() {
        let provider = AzureOpenAIProvider::new(
            "test-key".to_string(),
            "my-resource".to_string(),
            "gpt-4".to_string(),
        );

        assert_eq!(provider.context_window("gpt-4"), Some(8192));
        assert_eq!(provider.context_window("gpt-4-32k"), Some(32768));
        assert_eq!(provider.context_window("gpt-35-turbo-16k"), Some(16384));
    }

    #[test]
    fn test_azure_cost_calculation() {
        let provider = AzureOpenAIProvider::new(
            "test-key".to_string(),
            "my-resource".to_string(),
            "gpt-4".to_string(),
        );

        let cost = provider.calculate_cost("gpt-4", 1000, 1000);
        assert!((cost - 0.09).abs() < 0.001); // $0.03 + $0.06 = $0.09

        let cost_turbo = provider.calculate_cost("gpt-35-turbo", 1000, 1000);
        assert!((cost_turbo - 0.0035).abs() < 0.0001); // $0.0015 + $0.002
    }

    #[test]
    fn test_azure_supported_models() {
        let provider = AzureOpenAIProvider::new(
            "test-key".to_string(),
            "my-resource".to_string(),
            "gpt-4".to_string(),
        );

        let models = provider.supported_models();
        assert!(models.contains(&"gpt-4".to_string()));
        assert!(models.contains(&"gpt-35-turbo".to_string()));
    }
}
