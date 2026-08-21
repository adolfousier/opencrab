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

// ── fallback-chain entries that name nothing ──────────────────────────

/// A config whose custom providers are `defined`, with `chain` as the
/// fallback provider list.
fn config_with_chain(defined: &[&str], chain: &[&str]) -> Config {
    use std::collections::BTreeMap;
    let mut custom = BTreeMap::new();
    for name in defined {
        custom.insert(
            (*name).to_string(),
            ProviderConfig {
                enabled: true,
                api_key: Some("key".into()),
                base_url: Some("https://example.invalid/v1".into()),
                ..Default::default()
            },
        );
    }
    Config {
        providers: ProviderConfigs {
            custom: Some(custom),
            fallback: Some(crate::config::types::FallbackProviderConfig {
                enabled: true,
                providers: chain.iter().map(|s| (*s).to_string()).collect(),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    }
}

#[test]
fn a_chain_entry_naming_nothing_is_reported() {
    // The chain skips it and the next provider answers, so without this the
    // user sees a model they never picked and no reason for it.
    let cfg = config_with_chain(&["modelscope-qwen37-max"], &["modelscope-qwen37max"]);
    let warns = startup_warnings(&cfg, None);
    assert!(
        warns
            .iter()
            .any(|w| w.contains("modelscope-qwen37max") && w.contains("skipped silently")),
        "expected the dangling entry to be reported, got {warns:?}"
    );
}

#[test]
fn a_separator_slip_names_the_provider_that_was_meant() {
    // One missing hyphen was the real case; the letters and digits match
    // exactly, so naming the intended provider costs no guesswork.
    let cfg = config_with_chain(&["modelscope-qwen37-max"], &["modelscope-qwen37max"]);
    let warns = startup_warnings(&cfg, None);
    assert!(
        warns
            .iter()
            .any(|w| w.contains("did you mean \"modelscope-qwen37-max\"")),
        "expected the near match to be suggested, got {warns:?}"
    );
}

#[test]
fn a_chain_of_configured_providers_is_quiet() {
    let cfg = config_with_chain(&["alpha", "beta"], &["alpha", "beta"]);
    let warns = startup_warnings(&cfg, None);
    assert!(
        !warns.iter().any(|w| w.contains("providers.fallback")),
        "a correct chain must not warn, got {warns:?}"
    );
}

#[test]
fn an_unrelated_name_is_reported_without_a_guess() {
    // No near match exists, so the warning must not invent one.
    let cfg = config_with_chain(&["alpha"], &["something-else-entirely"]);
    let warns = startup_warnings(&cfg, None);
    let hit = warns
        .iter()
        .find(|w| w.contains("something-else-entirely"))
        .expect("the dangling entry must still be reported");
    assert!(!hit.contains("did you mean"), "must not guess, got {hit}");
}

// ── reaching the user, not just the log ───────────────────────────────

use crate::config::startup_checks::{
    channel_notice_for, clear_startup_warnings_for_test, record_startup_warnings,
    recorded_startup_warnings,
};

/// The store is process-global, so these serialize against each other.
static NOTICE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serialized_notice() -> std::sync::MutexGuard<'static, ()> {
    let guard = NOTICE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_startup_warnings_for_test();
    guard
}

#[test]
fn recorded_warnings_survive_for_a_later_surface() {
    // The TUI builds after the check runs; draining on first read would leave
    // whichever surface came up second with nothing.
    let _guard = serialized_notice();
    record_startup_warnings(&["config: something is off".to_string()]);
    assert_eq!(recorded_startup_warnings().len(), 1);
    assert_eq!(
        recorded_startup_warnings().len(),
        1,
        "reading must not drain"
    );
}

#[test]
fn a_chat_is_told_once_and_not_again() {
    let _guard = serialized_notice();
    record_startup_warnings(&["config: something is off".to_string()]);

    let first = channel_notice_for("telegram:-100123");
    assert!(first.is_some_and(|n| n.contains("something is off")));
    assert!(
        channel_notice_for("telegram:-100123").is_none(),
        "repeating it on every message would be worse than the silence it replaces"
    );
}

#[test]
fn each_chat_gets_told_separately() {
    let _guard = serialized_notice();
    record_startup_warnings(&["config: something is off".to_string()]);
    assert!(channel_notice_for("telegram:-100123").is_some());
    assert!(
        channel_notice_for("telegram:-100999").is_some(),
        "a second chat has not been told yet"
    );
}

#[test]
fn a_clean_config_says_nothing_at_all() {
    let _guard = serialized_notice();
    assert!(channel_notice_for("telegram:-100123").is_none());
}
