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
//! The user-facing `/models` on channels was never affected by that half — it
//! goes through `direct_model_switch`, which already parsed the pair correctly.
//!
//! Both surfaces did share one flaw: they accepted a prefix that merely named a
//! provider this software supports, rather than one the user has configured.
//! `anthropic` is also an OpenRouter vendor, so `anthropic/claude-sonnet-4`
//! resolved to a section that need not exist. Both now ask `is_declared`, so
//! the same input means the provider when you have one and is a clear error
//! when you do not.

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
fn an_undeclared_prefix_is_an_error_that_names_the_fix() {
    // A vendor the user has not configured as a provider. Guessing either way
    // is wrong, so say so and show the qualified form.
    let mut cfg = Config::default();
    cfg.providers.openrouter = Some(provider(true, Some("k")));
    let err = resolve_model_target(&cfg, "tencent/hy3:free").expect_err("tencent is not declared");
    assert!(err.contains("tencent"), "must name the prefix: {err}");
    assert!(
        err.contains("openrouter/tencent/hy3:free"),
        "must show the qualified form that works: {err}"
    );
}

#[test]
fn the_vendor_provider_collision_is_settled_by_what_the_user_declared() {
    // `anthropic` is both a provider id and an OpenRouter vendor. Asking the
    // registry says "provider" every time, which routes a valid OpenRouter id
    // at a section that may not exist. Asking THIS config resolves it.
    let mut cfg = Config::default();
    cfg.providers.openrouter = Some(provider(true, Some("k")));

    // Anthropic not configured → this is an OpenRouter model id, and the user
    // is told how to say so rather than being silently misrouted.
    let err = resolve_model_target(&cfg, "anthropic/claude-sonnet-4")
        .expect_err("anthropic has no section here");
    assert!(
        err.contains("openrouter/anthropic/claude-sonnet-4"),
        "{err}"
    );

    // The qualified form always works, and only the first slash splits.
    let (p, model) =
        resolve_model_target(&cfg, "openrouter/anthropic/claude-sonnet-4").expect("must resolve");
    assert_eq!(p, "openrouter");
    assert_eq!(model, "anthropic/claude-sonnet-4");

    // Anthropic configured → the same input now means the provider, because
    // the user has one.
    cfg.providers.anthropic = Some(provider(true, Some("k")));
    let (p, model) = resolve_model_target(&cfg, "anthropic/claude-sonnet-4").expect("must resolve");
    assert_eq!(p, "anthropic");
    assert_eq!(model, "claude-sonnet-4");
}

#[test]
fn declared_but_disabled_still_counts_as_a_prefix() {
    // Naming a provider is how you point at one that is not active. Requiring
    // `enabled` would break exactly the case the prefix exists for.
    let mut cfg = Config::default();
    cfg.providers.openrouter = Some(provider(true, Some("k")));
    cfg.providers.anthropic = Some(provider(false, None));
    let (p, _) = resolve_model_target(&cfg, "anthropic/claude-sonnet-4").expect("must resolve");
    assert_eq!(p, "anthropic");
}

#[test]
fn an_alias_spelling_of_a_declared_provider_is_recognised() {
    // Config sections use `claude_cli`; ids use `claude-cli`. Both must land.
    let mut cfg = Config::default();
    cfg.providers.claude_cli = Some(provider(true, None));
    assert!(cfg.providers.is_declared("claude-cli"));
    assert!(cfg.providers.is_declared("claude_cli"));
    assert!(!cfg.providers.is_declared("codex-cli"), "not declared here");
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
