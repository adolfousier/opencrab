//! Tests for the startup config diagnostics (#477): ignored
//! working_directory keys are flagged (they are not in the schema, serde
//! drops them silently), and default_provider without force_default gets
//! the escape-hatch hint.

use crate::config::startup_checks::startup_warnings;
use crate::config::{Config, ProviderConfig, ProviderConfigs};

fn config_with_default(dp: Option<&str>, force: bool) -> Config {
    Config {
        providers: ProviderConfigs {
            minimax: Some(ProviderConfig {
                enabled: true,
                api_key: Some("key".into()),
                default_model: Some("MiniMax-M3".into()),
                force_default: force,
                ..Default::default()
            }),
            ..Default::default()
        },
        agent: crate::config::AgentConfig {
            default_provider: dp.map(str::to_string),
            ..Default::default()
        },
        ..Default::default()
    }
}

#[test]
fn clean_config_produces_no_warnings() {
    let warns = startup_warnings(&config_with_default(None, false), Some("[agent]\n"));
    assert!(warns.is_empty(), "{warns:?}");
}

#[test]
fn ignored_working_directory_key_is_flagged() {
    let raw = "[agent]\nworking_directory = \"/nonexistent/void\"\n";
    let warns = startup_warnings(&config_with_default(None, false), Some(raw));
    assert_eq!(warns.len(), 1);
    assert!(warns[0].contains("has NO effect"));
    assert!(warns[0].contains("does not exist on disk"));
}

#[test]
fn existing_dir_still_flags_the_ignored_key_without_missing_note() {
    let raw = "working_directory = \"/tmp\"\n";
    let warns = startup_warnings(&config_with_default(None, false), Some(raw));
    assert_eq!(warns.len(), 1);
    assert!(warns[0].contains("has NO effect"));
    assert!(!warns[0].contains("does not exist on disk"));
}

#[test]
fn default_provider_without_force_default_hints_the_flag() {
    let warns = startup_warnings(&config_with_default(Some("minimax"), false), None);
    assert_eq!(warns.len(), 1);
    assert!(warns[0].contains("NEW sessions only"));
    assert!(warns[0].contains("force_default = true"));
    assert!(warns[0].contains("[providers.minimax]"));
}

#[test]
fn default_provider_with_force_default_stays_quiet() {
    let warns = startup_warnings(&config_with_default(Some("minimax"), true), None);
    assert!(warns.is_empty(), "{warns:?}");
}

#[test]
fn unknown_default_provider_is_flagged() {
    let warns = startup_warnings(&config_with_default(Some("nonexistent"), false), None);
    assert_eq!(warns.len(), 1);
    assert!(warns[0].contains("does not match any configured provider section"));
}
