//! Regression (#939): `/models` resolves which provider it is writing to from
//! the provider registry, never from a hardcoded list, and says so when it
//! cannot.
//!
//! The old code tested six providers by name out of the twenty-two the registry
//! knows. A user on any of the other sixteen fell through to "first enabled
//! custom provider" and had `default_model` written into an unrelated section,
//! silently. The report described the custom-vs-custom case; the built-in
//! fall-through was the larger half.
//!
//! The user-facing `/models` on channels was never affected — it goes through
//! `direct_model_switch`, which already parsed the pair correctly. This covers
//! the agent-tool path, which duplicated the decision and got it wrong.

use crate::brain::tools::slash_command::{resolve_model_target, section_for_provider};
use crate::config::{Config, ProviderConfig};

fn provider(enabled: bool, key: Option<&str>) -> ProviderConfig {
    ProviderConfig {
        enabled,
        api_key: key.map(|k| k.to_string()),
        ..Default::default()
    }
}

fn custom(enabled: bool, base_url: &str) -> ProviderConfig {
    ProviderConfig {
        enabled,
        base_url: Some(base_url.to_string()),
        ..Default::default()
    }
}

/// A config with two declared custom providers and no built-in enabled.
fn config_with_customs() -> Config {
    let mut cfg = Config::default();
    let mut map = std::collections::BTreeMap::new();
    map.insert(
        "lm-studio".to_string(),
        custom(true, "http://localhost:1234/v1"),
    );
    map.insert(
        "surplus".to_string(),
        custom(false, "https://surplus.example/v1"),
    );
    cfg.providers.custom = Some(map);
    cfg
}

#[test]
fn an_explicit_custom_prefix_wins_over_the_enabled_custom_provider() {
    // The reported case. `lm-studio` is the enabled one and sorts first, so
    // "first enabled custom provider" picked it and ignored the prefix.
    let cfg = config_with_customs();
    let (provider, model) =
        resolve_model_target(&cfg, "surplus/gpt-5.6-luna").expect("a declared prefix must resolve");
    assert_eq!(provider, "surplus");
    assert_eq!(model, "gpt-5.6-luna");
    assert_eq!(
        section_for_provider(&cfg, &provider).as_deref(),
        Some("providers.custom.surplus"),
        "the model must be written into the provider the user named"
    );
}

#[test]
fn a_vendor_prefix_that_is_not_a_provider_stays_part_of_the_model_name() {
    // OpenRouter ids are `vendor/model`. A vendor the registry does not know
    // must not be mistaken for a provider, or a valid model id would be routed
    // to a provider nobody asked for.
    let mut cfg = Config::default();
    cfg.providers.openrouter = Some(provider(true, Some("k")));
    let (p, model) = resolve_model_target(&cfg, "tencent/hy3:free")
        .expect("must fall back to the active provider");
    assert_eq!(p, "openrouter", "'tencent' is a vendor, not a provider");
    assert_eq!(
        model, "tencent/hy3:free",
        "the full id must survive as the model name"
    );
}

#[test]
fn a_registry_provider_name_before_the_slash_is_always_a_prefix() {
    // Documents a genuine ambiguity rather than hiding it: `anthropic` is both
    // a provider id and an OpenRouter vendor, and the prefix wins.
    //
    // This matches `direct_model_switch`, the path a user's `/models` actually
    // takes, whose own error text tells them to write `<provider>/<model>`.
    // Making the tool path differ would rebuild the split this change exists to
    // remove. To set an OpenRouter model whose vendor collides with a provider
    // name, write the provider explicitly: `openrouter/anthropic/claude-x`.
    let mut cfg = Config::default();
    cfg.providers.openrouter = Some(provider(true, Some("k")));
    let (p, model) = resolve_model_target(&cfg, "anthropic/claude-sonnet-4").expect("must resolve");
    assert_eq!(p, "anthropic");
    assert_eq!(model, "claude-sonnet-4");

    // The escape hatch works, and only the first slash splits.
    let (p, model) =
        resolve_model_target(&cfg, "openrouter/anthropic/claude-sonnet-4").expect("must resolve");
    assert_eq!(p, "openrouter");
    assert_eq!(model, "anthropic/claude-sonnet-4");
}

#[test]
fn only_the_first_slash_splits_a_pair() {
    // Nested vendor paths are common; the model half keeps its own slashes.
    let cfg = config_with_customs();
    let (p, model) = resolve_model_target(&cfg, "surplus/vendor/model:tag").expect("must resolve");
    assert_eq!(p, "surplus");
    assert_eq!(model, "vendor/model:tag");
}

#[test]
fn a_bare_model_applies_to_the_active_provider_not_a_hardcoded_one() {
    // A provider the old six-name ladder never mentioned. It used to fall
    // through to the custom branch and write into a custom section instead.
    let mut cfg = config_with_customs();
    cfg.providers.qwen = Some(provider(true, Some("k")));
    let (p, model) = resolve_model_target(&cfg, "qwen3-max").expect("must resolve");
    assert_eq!(
        p, "qwen",
        "the active provider must be the registry's answer, not a name from a hardcoded list"
    );
    assert_eq!(model, "qwen3-max");
    assert_eq!(
        section_for_provider(&cfg, &p).as_deref(),
        Some("providers.qwen"),
    );
}

#[test]
fn no_configured_provider_is_an_error_rather_than_a_guess() {
    // The point of the change: fail loudly instead of picking something.
    let cfg = Config::default();
    let err = resolve_model_target(&cfg, "some-model").expect_err("nothing is configured");
    assert!(
        err.contains("No active provider"),
        "the error must say what is wrong: {err}"
    );
    assert!(
        err.contains("some-model"),
        "and name what it could not place: {err}"
    );
}

#[test]
fn an_enabled_but_keyless_provider_does_not_capture_the_write() {
    // The ladder tested `enabled` alone, so a keyless section outranked a
    // working one. active_provider_and_model requires the key.
    let mut cfg = Config::default();
    cfg.providers.anthropic = Some(provider(true, None));
    cfg.providers.qwen = Some(provider(true, Some("k")));
    let (p, _) = resolve_model_target(&cfg, "some-model").expect("a usable provider exists");
    assert_eq!(
        p, "qwen",
        "a provider that cannot serve a request must not receive the model"
    );
}

#[test]
fn a_custom_prefix_resolves_even_when_that_provider_is_disabled() {
    // Naming a provider explicitly is the user saying where the value goes.
    // Requiring it to already be enabled would make the prefix useless for
    // exactly the case it exists for: pointing at a different provider.
    let cfg = config_with_customs();
    let (p, _) = resolve_model_target(&cfg, "surplus/x").expect("must resolve");
    assert_eq!(p, "surplus");
}

#[test]
fn an_unknown_provider_id_has_no_section_rather_than_a_default_one() {
    let cfg = Config::default();
    assert_eq!(
        section_for_provider(&cfg, "not-a-provider"),
        None,
        "an unknown provider must not resolve to some fallback section"
    );
}

#[test]
fn built_in_and_custom_sections_both_resolve() {
    let cfg = config_with_customs();
    assert_eq!(
        section_for_provider(&cfg, "anthropic").as_deref(),
        Some("providers.anthropic")
    );
    assert_eq!(
        section_for_provider(&cfg, "custom:lm-studio").as_deref(),
        Some("providers.custom.lm-studio"),
        "the custom: prefix form used by active_provider_and_model must resolve"
    );
    assert_eq!(
        section_for_provider(&cfg, "lm-studio").as_deref(),
        Some("providers.custom.lm-studio"),
        "and so must the bare form a user types as a prefix"
    );
}
