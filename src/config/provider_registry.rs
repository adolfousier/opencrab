// provider registry client integration for OpenCrabs
// Replaces the planned Catwalk integration with provider registry

use super::registry_client::{Provider, RegistryClient};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Configuration for provider registry provider registry client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRegistryConfig {
    /// Enable provider registry integration for automatic provider discovery
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Base URL of the provider registry server
    /// Default: http://localhost:8080
    #[serde(default = "default_base_url")]
    pub base_url: String,

    /// Enable automatic provider updates on startup
    #[serde(default = "default_auto_update")]
    pub auto_update: bool,

    /// Update interval in seconds (0 = only on startup)
    #[serde(default = "default_update_interval")]
    pub update_interval_seconds: u64,
}

fn default_enabled() -> bool {
    true
}

fn default_base_url() -> String {
    "http://localhost:8080".to_string()
}

fn default_auto_update() -> bool {
    true
}

fn default_update_interval() -> u64 {
    3600 // 1 hour
}

impl Default for ProviderRegistryConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            base_url: default_base_url(),
            auto_update: default_auto_update(),
            update_interval_seconds: default_update_interval(),
        }
    }
}

/// provider registry client wrapper for OpenCrabs
pub struct ProviderRegistry {
    client: RegistryClient,
    config: ProviderRegistryConfig,
}

impl ProviderRegistry {
    /// Create a new provider registry integration instance
    pub fn new(config: ProviderRegistryConfig) -> Result<Self> {
        let client = RegistryClient::new(&config.base_url);

        Ok(Self { client, config })
    }

    /// Fetch all providers from provider registry registry
    pub async fn fetch_providers(&self) -> Result<Vec<Provider>> {
        self.client
            .get_providers()
            .await
            .context("Failed to fetch providers from provider registry")
    }

    /// Check if provider registry server is healthy
    pub async fn health_check(&self) -> Result<bool> {
        self.client
            .health_check()
            .await
            .context("Failed to check provider registry health")
    }

    /// Get provider by ID
    pub async fn get_provider(&self, provider_id: &str) -> Result<Option<Provider>> {
        let providers = self.fetch_providers().await?;
        Ok(providers.into_iter().find(|p| p.id == provider_id))
    }

    /// Get all available model IDs across all providers
    pub async fn get_all_model_ids(&self) -> Result<Vec<String>> {
        let providers = self.fetch_providers().await?;
        let model_ids: Vec<String> = providers
            .into_iter()
            .flat_map(|p| p.models.into_iter().map(|m| m.id))
            .collect();

        Ok(model_ids)
    }

    /// Check if a specific provider is available
    pub async fn is_provider_available(&self, provider_id: &str) -> Result<bool> {
        Ok(self.get_provider(provider_id).await?.is_some())
    }

    /// Get the configuration
    pub fn config(&self) -> &ProviderRegistryConfig {
        &self.config
    }
}
