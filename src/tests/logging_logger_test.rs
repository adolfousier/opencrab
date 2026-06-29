use crate::logging::logger::*;
use tracing::Level;

#[test]
fn test_log_config_default() {
    let config = LogConfig::default();
    assert!(!config.debug_mode);
    assert_eq!(config.log_level, Level::INFO);
    assert!(!config.console_output);
    assert_eq!(config.log_prefix, "opencrabs");
}

#[test]
fn test_log_config_with_debug() {
    let config = LogConfig::new().with_debug_mode(true);
    assert!(config.debug_mode);
    assert_eq!(config.log_level, Level::DEBUG);
}

#[test]
fn test_log_config_builder() {
    let config = LogConfig::new()
        .with_log_level(Level::TRACE)
        .with_console_output(true)
        .with_log_prefix("test".to_string());

    assert_eq!(config.log_level, Level::TRACE);
    assert!(config.console_output);
    assert_eq!(config.log_prefix, "test");
}

#[test]
fn test_log_dir_in_home_opencrabs_folder() {
    let config = LogConfig::default();
    let log_dir_str = config.log_dir.to_string_lossy();
    assert!(log_dir_str.contains(".opencrabs"));
    assert!(log_dir_str.contains("logs"));
    // Should be under home dir, not cwd
    if let Some(home) = dirs::home_dir() {
        assert!(config.log_dir.starts_with(&home));
    }
}
