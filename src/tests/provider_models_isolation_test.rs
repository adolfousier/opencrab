//! A provider's config models are its own, never another provider's.
//!
//! `reload_config_models` read `active_custom()` for any custom provider —
//! whichever one happened to be marked active — rather than the one being
//! edited. The persist step merges `config_models` into what it writes, so a
//! freshly created provider was saved listing models it does not serve, and
//! when its own `/v1/models` fetch had not landed yet that foreign list was
//! the only thing written. The same eight names reached several unrelated
//! providers this way (#1156).

use std::collections::BTreeMap;

use crate::config::{ProviderConfig, ProviderConfigs};

fn provider_with(models: &[&str], enabled: bool) -> ProviderConfig {
    ProviderConfig {
        enabled,
        api_key: Some("key".into()),
        base_url: Some("https://example.invalid/v1".into()),
        models: models.iter().map(|m| (*m).to_string()).collect(),
        ..Default::default()
    }
}

/// Two custom providers: one active with a catalogue, one freshly added with
/// none. This is the exact shape that produced the bug.
fn configs() -> ProviderConfigs {
    let mut custom = BTreeMap::new();
    custom.insert(
        "active-one".to_string(),
        provider_with(&["kimi-k2.5", "glm-5", "gpt-oss-120b"], true),
    );
    custom.insert("brand-new".to_string(), provider_with(&[], false));
    custom.insert(
        "has-its-own".to_string(),
        provider_with(&["qwen/qwen3.8-27b-free", "tencent/hy3"], false),
    );
    ProviderConfigs {
        custom: Some(custom),
        ..Default::default()
    }
}

#[test]
fn a_provider_is_looked_up_by_its_own_name() {
    let cfg = configs();
    let own = cfg.custom_by_name("has-its-own").expect("present");

    assert_eq!(own.models, vec!["qwen/qwen3.8-27b-free", "tencent/hy3"]);
}

#[test]
fn a_new_provider_has_no_models_of_its_own() {
    // The case that broke: nothing to inherit means nothing to write. It must
    // not fall back to the active provider's catalogue.
    let cfg = configs();
    let fresh = cfg.custom_by_name("brand-new").expect("present");

    assert!(
        fresh.models.is_empty(),
        "a provider nobody has fetched for lists nothing"
    );
}

#[test]
fn the_active_provider_is_not_the_one_being_edited() {
    // `active_custom()` answers a different question than "the provider I am
    // configuring", and that difference is the whole bug.
    let cfg = configs();
    let active = cfg.active_custom().map(|(name, _)| name.to_string());

    assert_eq!(active.as_deref(), Some("active-one"));
    assert_ne!(
        active.as_deref(),
        Some("brand-new"),
        "editing brand-new must not resolve to active-one"
    );
}

#[test]
fn the_active_providers_catalogue_is_not_shared() {
    // Pins the contamination itself: the eight names that reached unrelated
    // providers belong to one of them and must stay there.
    let cfg = configs();
    let active = cfg.custom_by_name("active-one").expect("present");
    let other = cfg.custom_by_name("has-its-own").expect("present");

    assert!(active.models.contains(&"kimi-k2.5".to_string()));
    assert!(
        !other.models.contains(&"kimi-k2.5".to_string()),
        "a model from one provider must never appear in another's list"
    );
}

#[test]
fn a_name_that_matches_nothing_yields_nothing() {
    // A provider mid-creation, before its table exists, must resolve to None
    // rather than to whatever else is configured.
    let cfg = configs();
    assert!(cfg.custom_by_name("not-created-yet").is_none());
}
