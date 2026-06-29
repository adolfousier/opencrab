use super::*;
use crate::config::Config;
use std::path::PathBuf;
use tempfile::NamedTempFile;

#[test]
fn test_default_config() {
    let config = Config::default();
    assert!(config.crabrace.enabled);
    assert_eq!(config.logging.level, "info");
    assert!(!config.debug.debug_lsp);
    assert!(!config.debug.profiling);
}

#[test]
fn test_config_validation() {
    let config = Config::default();
    assert!(config.validate().is_ok());
}

#[test]
fn test_config_validation_invalid_log_level() {
    let mut config = Config::default();
    config.logging.level = "invalid".to_string();
    assert!(config.validate().is_err());
}

#[test]
fn test_config_validation_empty_crabrace_url() {
    let mut config = Config::default();
    config.crabrace.base_url = String::new();
    assert!(config.validate().is_err());
}

#[test]
fn test_config_from_toml() {
    let toml_content = r#"
[database]
path = "/custom/path/db.sqlite"

[logging]
level = "debug"

[debug]
debug_lsp = true
profiling = true

[crabrace]
enabled = false
        "#;

    let config: Config = toml::from_str(toml_content).unwrap();
    assert_eq!(
        config.database.path,
        PathBuf::from("/custom/path/db.sqlite")
    );
    assert_eq!(config.logging.level, "debug");
    assert!(config.debug.debug_lsp);
    assert!(config.debug.profiling);
    assert!(!config.crabrace.enabled);
}

#[test]
fn test_config_save_and_load() {
    let temp_file = NamedTempFile::new().unwrap();
    let config = Config::default();

    // Save config
    config.save(temp_file.path()).unwrap();

    // Load config back
    let contents = std::fs::read_to_string(temp_file.path()).unwrap();
    let loaded_config: Config = toml::from_str(&contents).unwrap();

    assert_eq!(loaded_config.logging.level, config.logging.level);
    assert_eq!(loaded_config.crabrace.enabled, config.crabrace.enabled);
}

#[test]
fn test_config_from_toml_overrides() {
    let toml_content = r#"
[logging]
level = "trace"

[debug]
debug_lsp = true
profiling = true

[database]
path = "/tmp/test.db"
        "#;

    let config: Config = toml::from_str(toml_content).unwrap();
    assert_eq!(config.logging.level, "trace");
    assert!(config.debug.debug_lsp);
    assert!(config.debug.profiling);
    assert_eq!(config.database.path, PathBuf::from("/tmp/test.db"));
}

#[test]
fn test_provider_config_from_toml() {
    let toml_content = r#"
[providers.anthropic]
enabled = true
api_key = "test-anthropic-key"
default_model = "claude-opus-4-6"

[providers.openai]
enabled = true
api_key = "test-openai-key"
        "#;

    let config: Config = toml::from_str(toml_content).unwrap();

    assert!(config.providers.anthropic.is_some());
    let anthropic = config.providers.anthropic.as_ref().unwrap();
    assert_eq!(anthropic.api_key, Some("test-anthropic-key".to_string()));
    assert_eq!(anthropic.default_model, Some("claude-opus-4-6".to_string()));

    assert!(config.providers.openai.is_some());
    assert_eq!(
        config.providers.openai.as_ref().unwrap().api_key,
        Some("test-openai-key".to_string())
    );
}

#[test]
fn test_system_config_path() {
    let path = Config::system_config_path();
    assert!(path.is_some());
    let path = path.unwrap();
    assert!(path.to_string_lossy().contains("opencrabs"));
    assert!(path.to_string_lossy().ends_with("config.toml"));
}

#[test]
fn test_local_config_path() {
    let path = Config::local_config_path();
    assert_eq!(path, PathBuf::from("./opencrabs.toml"));
}

#[test]
fn test_debug_config_default() {
    let debug = DebugConfig::default();
    assert!(!debug.debug_lsp);
    assert!(!debug.profiling);
}

#[test]
fn test_provider_configs_default() {
    let providers = ProviderConfigs::default();
    assert!(providers.anthropic.is_none());
    assert!(providers.openai.is_none());
    assert!(providers.gemini.is_none());
    assert!(providers.bedrock.is_none());
    assert!(providers.vertex.is_none());
}

#[test]
fn test_database_config_default() {
    let db_config = DatabaseConfig::default();
    assert!(!db_config.path.as_os_str().is_empty());
}

#[test]
fn test_logging_config_default() {
    let logging = LoggingConfig::default();
    assert_eq!(logging.level, "info");
    assert!(logging.file.is_none());
}

#[test]
fn test_agent_config_default() {
    let agent = AgentConfig::default();
    assert_eq!(agent.approval_policy, "auto-always");
    assert_eq!(agent.max_concurrent, 4);
}

#[test]
fn test_agent_config_from_toml() {
    let toml_content = r#"
[agent]
approval_policy = "auto-always"
max_concurrent = 8
        "#;

    let config: Config = toml::from_str(toml_content).unwrap();
    assert_eq!(config.agent.approval_policy, "auto-always");
    assert_eq!(config.agent.max_concurrent, 8);
}

#[test]
fn test_agent_config_defaults_when_absent() {
    // Config without [agent] section should use defaults
    let toml_content = r#"
[logging]
level = "info"
        "#;

    let config: Config = toml::from_str(toml_content).unwrap();
    assert_eq!(config.agent.approval_policy, "auto-always");
    assert_eq!(config.agent.max_concurrent, 4);
}

#[test]
fn test_write_key_creates_and_updates() {
    let dir = tempfile::TempDir::new().unwrap();
    let config_path = dir.path().join("config.toml");

    // Write initial content
    fs::write(&config_path, "[logging]\nlevel = \"info\"\n").unwrap();

    // Use write_key-style logic (can't call write_key directly since it
    // uses system_config_path, but we test the merge logic)
    let content = fs::read_to_string(&config_path).unwrap();
    let mut doc: toml::Value = toml::from_str(&content).unwrap();
    let table = doc.as_table_mut().unwrap();

    // Add a new section
    table.insert(
        "agent".to_string(),
        toml::Value::Table({
            let mut m = toml::map::Map::new();
            m.insert(
                "approval_policy".to_string(),
                toml::Value::String("auto-session".to_string()),
            );
            m
        }),
    );

    let output = toml::to_string_pretty(&doc).unwrap();
    fs::write(&config_path, &output).unwrap();

    // Verify it round-trips
    let content = fs::read_to_string(&config_path).unwrap();
    let loaded: Config = toml::from_str(&content).unwrap();
    assert_eq!(loaded.agent.approval_policy, "auto-session");
    assert_eq!(loaded.logging.level, "info");
}

#[test]
fn test_config_save_with_agent_section() {
    let temp_file = NamedTempFile::new().unwrap();
    let mut config = Config::default();
    config.agent.approval_policy = "auto-always".to_string();
    config.agent.max_concurrent = 2;

    config.save(temp_file.path()).unwrap();

    let contents = fs::read_to_string(temp_file.path()).unwrap();
    let loaded: Config = toml::from_str(&contents).unwrap();
    assert_eq!(loaded.agent.approval_policy, "auto-always");
    assert_eq!(loaded.agent.max_concurrent, 2);
}
