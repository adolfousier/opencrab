//! RSI provider resolution must honor the user's declared pair and
//! fallback-chain order, not a hardcoded registry walk (#977, see also #968).
//!
//! The old code resolved straight through `active_provider_and_model()`,
//! whose walk starts at xiaomi: any install with an enabled keyed xiaomi
//! section ran RSI on that expensive 1M-window pair regardless of what the
//! user actually chatted on. Fixtures are synthetic configs; no keys or
//! user identifiers.

use crate::brain::rsi::resolve_rsi_pair;
use crate::config::types::Config;

fn config(json_str: &str) -> Config {
    serde_json::from_str(json_str).unwrap()
}

#[test]
fn explicit_self_improvement_provider_wins_as_is() {
    // The override is trusted without a health check; the #469 runtime
    // fallback demotes it if creation fails.
    let cfg = config(r#"{ "agent": { "self_improvement_provider": "minimax" } }"#);
    assert_eq!(resolve_rsi_pair(&cfg).provider, "minimax");
}

#[test]
fn declared_session_default_beats_the_registry_walk() {
    // The regression (#977): enabled keyed xiaomi won via registry order.
    // A healthy declared session default must beat it.
    let cfg = config(
        r#"{
            "agent": { "default_provider": "claude_cli" },
            "providers": {
                "xiaomi": { "enabled": true, "api_key": "k" },
                "claude_cli": { "enabled": true }
            }
        }"#,
    );
    assert_eq!(resolve_rsi_pair(&cfg).provider, "claude_cli");
}

#[test]
fn unhealthy_declared_default_falls_through_to_the_chain() {
    // Declared but disabled is not healthy: keep walking.
    let cfg = config(
        r#"{
            "agent": { "default_provider": "opencode" },
            "providers": {
                "opencode": { "enabled": false },
                "xiaomi": { "enabled": true, "api_key": "k" },
                "fallback": { "enabled": true, "providers": ["xiaomi"] }
            }
        }"#,
    );
    assert_eq!(resolve_rsi_pair(&cfg).provider, "xiaomi");
}

#[test]
fn the_first_healthy_fallback_entry_wins_in_chain_order() {
    // Chain order is the user's, not the registry's: xiaomi first but
    // disabled, minimax healthy → minimax, even though anthropic is also
    // healthy and appears later.
    let cfg = config(
        r#"{
            "providers": {
                "xiaomi": { "enabled": false },
                "minimax": { "enabled": true, "api_key": "k" },
                "anthropic": { "enabled": true, "api_key": "k" },
                "fallback": {
                    "enabled": true,
                    "providers": ["xiaomi", "minimax", "anthropic"]
                }
            }
        }"#,
    );
    assert_eq!(resolve_rsi_pair(&cfg).provider, "minimax");
}

#[test]
fn keyless_keyed_providers_are_not_healthy() {
    // The chain must skip providers that cannot authenticate: minimax
    // requires a key and has none, so anthropic takes it.
    let cfg = config(
        r#"{
            "providers": {
                "minimax": { "enabled": true },
                "anthropic": { "enabled": true, "api_key": "k" },
                "fallback": {
                    "enabled": true,
                    "providers": ["minimax", "anthropic"]
                }
            }
        }"#,
    );
    assert_eq!(resolve_rsi_pair(&cfg).provider, "anthropic");
}

#[test]
fn a_disabled_fallback_chain_is_skipped() {
    let cfg = config(
        r#"{
            "providers": {
                "minimax": { "enabled": true, "api_key": "k" },
                "xiaomi": { "enabled": true, "api_key": "k" },
                "fallback": { "enabled": false, "providers": ["minimax"] }
            }
        }"#,
    );
    assert_eq!(resolve_rsi_pair(&cfg).provider, "xiaomi");
}

#[test]
fn registry_walk_remains_the_last_resort() {
    // Nothing declared, no chain: the old behavior is the floor.
    let cfg = config(r#"{ "providers": { "xiaomi": { "enabled": true, "api_key": "k" } } }"#);
    assert_eq!(resolve_rsi_pair(&cfg).provider, "xiaomi");
}

// ------------------------------------------------------------- #1314

#[test]
fn a_custom_prefixed_override_resolves_to_the_section_name() {
    // The live report: "custom:glm-53-max" for [providers.custom.glm-53-max]
    // never ran a cycle; the bare name did.
    let cfg = config(
        r#"{
            "agent": { "self_improvement_provider": "custom:glm-53-max" },
            "providers": {
                "custom": { "glm-53-max": { "base_url": "http://localhost:1/v1", "api_key": "k" } }
            }
        }"#,
    );
    let pair = resolve_rsi_pair(&cfg);
    assert_eq!(pair.provider, "glm-53-max");
    assert_eq!(pair.model, None);
    assert!(pair.note.is_some(), "the correction must be reportable");
    assert_eq!(resolve_rsi_pair(&cfg).provider, "glm-53-max");
}

#[test]
fn a_provider_slash_model_override_lands_the_model_in_the_pair() {
    let cfg = config(
        r#"{
            "agent": { "self_improvement_provider": "minimax/MiniMax-M2" },
            "providers": { "minimax": { "enabled": true, "api_key": "k" } }
        }"#,
    );
    let pair = resolve_rsi_pair(&cfg);
    assert_eq!(pair.provider, "minimax");
    assert_eq!(pair.model.as_deref(), Some("MiniMax-M2"));
}

#[test]
fn a_bare_override_and_model_key_pass_through_unnoted() {
    let cfg = config(
        r#"{
            "agent": {
                "self_improvement_provider": "minimax",
                "self_improvement_model": "MiniMax-M2"
            },
            "providers": { "minimax": { "enabled": true, "api_key": "k" } }
        }"#,
    );
    let pair = resolve_rsi_pair(&cfg);
    assert_eq!(pair.provider, "minimax");
    assert_eq!(pair.model.as_deref(), Some("MiniMax-M2"));
    assert_eq!(pair.note, None);
}

#[test]
fn no_override_keeps_the_ladder_and_the_model_key() {
    let cfg = config(
        r#"{
            "agent": { "default_provider": "minimax", "self_improvement_model": "MiniMax-M2" },
            "providers": { "minimax": { "enabled": true, "api_key": "k" } }
        }"#,
    );
    let pair = resolve_rsi_pair(&cfg);
    assert_eq!(pair.provider, "minimax");
    assert_eq!(pair.model.as_deref(), Some("MiniMax-M2"));
    assert_eq!(pair.note, None);
}
