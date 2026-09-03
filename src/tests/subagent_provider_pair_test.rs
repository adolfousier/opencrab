//! Child-agent provider/model resolution (#1316): per-call beats config, and
//! the winning value is normalised the way the RSI key is (#1314).
//! Fixtures are synthetic; no keys or user identifiers.

use crate::brain::tools::subagent::provider_pair::{ChildPair, ProviderSource, child_pair};
use crate::config::Config;

fn config(json: &str) -> Config {
    serde_json::from_str(json).expect("fixture config")
}

fn with_custom() -> Config {
    config(
        r#"{
            "agent": {
                "subagent_provider": "custom:myprovider",
                "subagent_model": "some-model"
            },
            "providers": {
                "custom": { "myprovider": { "base_url": "http://localhost:1/v1", "api_key": "k" } }
            }
        }"#,
    )
}

#[test]
fn a_custom_prefixed_config_value_resolves_to_the_section_name() {
    let pair = child_pair(&with_custom(), None, None);
    assert_eq!(
        pair,
        ChildPair {
            provider: Some("myprovider".into()),
            model: Some("some-model".into()),
            source: ProviderSource::Config,
        }
    );
}

#[test]
fn a_per_call_provider_beats_config_and_is_normalised_too() {
    let cfg = config(
        r#"{
            "agent": { "subagent_provider": "minimax" },
            "providers": {
                "minimax": { "enabled": true, "api_key": "k" },
                "zhipu": { "enabled": true, "api_key": "k" }
            }
        }"#,
    );
    let pair = child_pair(&cfg, Some("zhipu/glm-5.3"), None);
    assert_eq!(pair.provider.as_deref(), Some("zhipu"));
    assert_eq!(pair.model.as_deref(), Some("glm-5.3"));
    assert_eq!(pair.source, ProviderSource::PerCall);
}

#[test]
fn a_per_call_model_wins_over_a_model_found_in_the_provider_value() {
    let cfg = config(
        r#"{
            "agent": {},
            "providers": { "zhipu": { "enabled": true, "api_key": "k" } }
        }"#,
    );
    let pair = child_pair(&cfg, Some("zhipu/glm-5.3"), Some("glm-5.3-air"));
    assert_eq!(pair.provider.as_deref(), Some("zhipu"));
    assert_eq!(pair.model.as_deref(), Some("glm-5.3-air"));
}

#[test]
fn nothing_configured_means_inherit_the_parent() {
    let cfg = config(r#"{ "agent": {}, "providers": {} }"#);
    let pair = child_pair(&cfg, None, None);
    assert_eq!(pair.provider, None);
    assert_eq!(pair.model, None);
    assert_eq!(pair.source, ProviderSource::Config);
}

#[test]
fn a_bare_name_passes_through_untouched() {
    let cfg = config(
        r#"{
            "agent": { "subagent_provider": "minimax", "subagent_model": "MiniMax-M2" },
            "providers": { "minimax": { "enabled": true, "api_key": "k" } }
        }"#,
    );
    let pair = child_pair(&cfg, None, None);
    assert_eq!(pair.provider.as_deref(), Some("minimax"));
    assert_eq!(pair.model.as_deref(), Some("MiniMax-M2"));
}
