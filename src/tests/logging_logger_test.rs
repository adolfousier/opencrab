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
fn effective_debug_logs_is_flag_or_config() {
    // #678: the --debug flag OR the config toggle enables debug file logging.
    // The flag always wins, so a config `false` can never silence `-d`.
    assert!(!effective_debug_logs(false, false), "neither → off");
    assert!(
        effective_debug_logs(false, true),
        "config alone enables it (non-technical user case)"
    );
    assert!(
        effective_debug_logs(true, false),
        "flag alone enables it even with config off"
    );
    assert!(effective_debug_logs(true, true), "both → on");
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
