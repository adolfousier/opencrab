use crate::config::Config;
use crate::config::provider_registry::ProviderRegistry;
use crate::config::provider_registry::ProviderRegistryConfig;
use crate::config::update::*;

#[test]
fn test_should_update_when_disabled() {
    let provider_registry_config = ProviderRegistryConfig {
        enabled: false,
        ..Default::default()
    };
    let provider_registry = ProviderRegistry::new(provider_registry_config.clone()).unwrap();
    let updater = ProviderUpdater::new(provider_registry);

    let config = Config {
        provider_registry: provider_registry_config,
        ..Default::default()
    };

    assert!(!updater.should_update(&config));
}

#[test]
fn test_should_update_when_never_updated() {
    let provider_registry_config = ProviderRegistryConfig {
        enabled: true,
        auto_update: true,
        ..Default::default()
    };
    let provider_registry = ProviderRegistry::new(provider_registry_config.clone()).unwrap();
    let updater = ProviderUpdater::new(provider_registry);

    let config = Config {
        provider_registry: provider_registry_config,
        ..Default::default()
    };

    assert!(updater.should_update(&config));
}

#[test]
fn test_update_result_success() {
    let result = UpdateResult::success(5);
    assert!(result.success);
    assert_eq!(result.providers_updated, 5);
    assert!(result.error.is_none());
}

#[test]
fn test_update_result_failure() {
    let result = UpdateResult::failure("Connection failed".to_string());
    assert!(!result.success);
    assert_eq!(result.providers_updated, 0);
    assert_eq!(result.error, Some("Connection failed".to_string()));
}
