use crate::config::provider_registry::*;
use tokio;

#[test]
fn test_default_config() {
    let config = ProviderRegistryConfig::default();
    assert!(config.enabled);
    assert_eq!(config.base_url, "http://localhost:8080");
    assert!(config.auto_update);
    assert_eq!(config.update_interval_seconds, 3600);
}

#[test]
fn test_create_integration() {
    let config = ProviderRegistryConfig::default();
    let integration = ProviderRegistry::new(config);
    assert!(integration.is_ok());
}

#[tokio::test]
async fn test_health_check() {
    // This test requires a running provider registry server
    let config = ProviderRegistryConfig::default();
    let integration = ProviderRegistry::new(config).unwrap();

    // Note: This will fail if server is not running
    // In production, handle this gracefully
    let _ = integration.health_check().await;
}
