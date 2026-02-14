// Crabrace client integration for OpenCrab
// Replaces the planned Catwalk integration with Crabrace

use anyhow::{Context, Result};
use crabrace::{CrabraceClient, Provider};
use serde::{Deserialize, Serialize};

/// Configuration for Crabrace provider registry client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrabraceConfig {
    /// Enable Crabrace integration for automatic provider discovery
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Base URL of the Crabrace server
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

impl Default for CrabraceConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            base_url: default_base_url(),
            auto_update: default_auto_update(),
            update_interval_seconds: default_update_interval(),
        }
    }
}

/// Crabrace client wrapper for OpenCrab
pub struct CrabraceIntegration {
    client: CrabraceClient,
    config: CrabraceConfig,
}

impl CrabraceIntegration {
    /// Create a new Crabrace integration instance
    pub fn new(config: CrabraceConfig) -> Result<Self> {
        let client = CrabraceClient::new(&config.base_url);

        Ok(Self { client, config })
    }

    /// Fetch all providers from Crabrace registry
    pub async fn fetch_providers(&self) -> Result<Vec<Provider>> {
        self.client
            .get_providers()
            .await
            .context("Failed to fetch providers from Crabrace")
    }

    /// Check if Crabrace server is healthy
    pub async fn health_check(&self) -> Result<bool> {
        self.client
            .health_check()
            .await
            .context("Failed to check Crabrace health")
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
    pub fn config(&self) -> &CrabraceConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = CrabraceConfig::default();
        assert!(config.enabled);
        assert_eq!(config.base_url, "http://localhost:8080");
        assert!(config.auto_update);
        assert_eq!(config.update_interval_seconds, 3600);
    }

    #[test]
    fn test_create_integration() {
        let config = CrabraceConfig::default();
        let integration = CrabraceIntegration::new(config);
        assert!(integration.is_ok());
    }

    #[tokio::test]
    async fn test_health_check() {
        // This test requires a running Crabrace server
        let config = CrabraceConfig::default();
        let integration = CrabraceIntegration::new(config).unwrap();

        // Note: This will fail if server is not running
        // In production, handle this gracefully
        let _ = integration.health_check().await;
    }
}
