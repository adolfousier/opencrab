use super::*;
use crate::config::crabrace::CrabraceConfig;

#[test]
fn test_should_update_when_disabled() {
    let crabrace_config = CrabraceConfig {
        enabled: false,
        ..Default::default()
    };
    let crabrace = CrabraceIntegration::new(crabrace_config.clone()).unwrap();
    let updater = ProviderUpdater::new(crabrace);

    let config = Config {
        crabrace: crabrace_config,
        ..Default::default()
    };

    assert!(!updater.should_update(&config));
}

#[test]
fn test_should_update_when_never_updated() {
    let crabrace_config = CrabraceConfig {
        enabled: true,
        auto_update: true,
        ..Default::default()
    };
    let crabrace = CrabraceIntegration::new(crabrace_config.clone()).unwrap();
    let updater = ProviderUpdater::new(crabrace);

    let config = Config {
        crabrace: crabrace_config,
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
