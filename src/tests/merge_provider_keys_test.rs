//! Regression tests for `merge_provider_keys`.
//!
//! Background: a key written to `keys.toml` under
//! `[providers.<name>] api_key = "…"` only takes effect at runtime if
//! `merge_provider_keys` has an explicit branch for `<name>`. Adding a
//! new top-level provider field on `ProviderConfigs` without adding a
//! corresponding merge branch causes a silent failure: the key is on
//! disk, the running config never sees it, and the provider factory
//! reports "API key missing" with no obvious cause.
//!
//! These tests pin the contract for the providers we ship today.

use crate::config::{ProviderConfig, ProviderConfigs, merge_provider_keys};

fn key_only(api_key: &str) -> ProviderConfig {
    ProviderConfig {
        api_key: Some(api_key.to_string()),
        ..Default::default()
    }
}

#[test]
fn opencode_api_key_from_keys_toml_lands_in_runtime_config() {
    // Repro for the v0.3.16 bug: /models writes `[providers.opencode]
    // api_key = "..."` to keys.toml, but on the next config reload
    // merge_provider_keys was missing an opencode branch — runtime
    // Config.providers.opencode.api_key stayed None, factory.rs
    // reported "API key missing", and the new selection silently
    // failed to take effect.
    let base = ProviderConfigs::default();
    let keys = ProviderConfigs {
        opencode: Some(key_only("oc_test_key")),
        ..Default::default()
    };

    let merged = merge_provider_keys(base, keys);
    let opencode = merged.opencode.expect("opencode entry created");
    assert_eq!(opencode.api_key.as_deref(), Some("oc_test_key"));
    assert!(
        opencode.enabled,
        "first-time keys.toml load should auto-enable opencode"
    );
}

#[test]
fn opencode_existing_config_disabled_state_is_preserved_on_key_merge() {
    // If config.toml has `enabled = false` for opencode but keys.toml
    // carries an api_key, the user's explicit disabled state wins —
    // we only auto-enable when there's no entry at all.
    let base = ProviderConfigs {
        opencode: Some(ProviderConfig {
            enabled: false,
            ..Default::default()
        }),
        ..Default::default()
    };
    let keys = ProviderConfigs {
        opencode: Some(key_only("oc_test_key")),
        ..Default::default()
    };

    let merged = merge_provider_keys(base, keys);
    let opencode = merged.opencode.expect("opencode entry preserved");
    assert_eq!(opencode.api_key.as_deref(), Some("oc_test_key"));
    assert!(
        !opencode.enabled,
        "user's explicit disabled state must not flip on key merge"
    );
}

#[test]
fn sentinel_placeholder_does_not_leak_into_runtime_config() {
    // /models uses `__EXISTING_KEY__` internally to mean "keep the
    // current key". The merge function must never propagate that
    // sentinel into the runtime config.
    let base = ProviderConfigs::default();
    let keys = ProviderConfigs {
        opencode: Some(key_only("__EXISTING_KEY__")),
        ..Default::default()
    };

    let merged = merge_provider_keys(base, keys);
    assert!(
        merged.opencode.is_none(),
        "sentinel must not create an opencode entry"
    );
}

#[test]
fn anthropic_openai_qwen_keys_still_merge_after_opencode_addition() {
    // Smoke test that the existing branches still work — protects
    // against accidental regressions when adding new branches.
    let base = ProviderConfigs::default();
    let keys = ProviderConfigs {
        anthropic: Some(key_only("ant_key")),
        openai: Some(key_only("oai_key")),
        qwen: Some(key_only("qwen_key")),
        ..Default::default()
    };

    let merged = merge_provider_keys(base, keys);
    assert_eq!(
        merged.anthropic.and_then(|c| c.api_key).as_deref(),
        Some("ant_key")
    );
    assert_eq!(
        merged.openai.and_then(|c| c.api_key).as_deref(),
        Some("oai_key")
    );
    assert_eq!(
        merged.qwen.and_then(|c| c.api_key).as_deref(),
        Some("qwen_key")
    );
}

#[test]
fn moonshot_api_key_from_keys_toml_lands_in_runtime_config() {
    // #610: the built-in Moonshot AI (Kimi) provider wrote its key to keys.toml
    // ("Wrote secret key [providers.moonshot].api_key") but merge_provider_keys
    // had no moonshot branch, so runtime config.providers.moonshot.api_key
    // stayed None and the factory reported "Moonshot AI enabled but API key
    // missing" right after setup.
    let base = ProviderConfigs::default();
    let keys = ProviderConfigs {
        moonshot: Some(key_only("sk-kimi-test")),
        ..Default::default()
    };
    let merged = merge_provider_keys(base, keys);
    let moonshot = merged.moonshot.expect("moonshot entry created");
    assert_eq!(moonshot.api_key.as_deref(), Some("sk-kimi-test"));
}
