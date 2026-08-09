//! RSI provider resolution must honor the user's declared pair and
//! fallback-chain order, not a hardcoded registry walk (#977, see also #968).
//!
//! The old code resolved straight through `active_provider_and_model()`,
//! whose walk starts at xiaomi: any install with an enabled keyed xiaomi
//! section ran RSI on that expensive 1M-window pair regardless of what the
//! user actually chatted on. Fixtures are synthetic configs; no keys or
//! user identifiers.

use crate::brain::rsi::resolve_rsi_provider;
use crate::config::types::Config;

fn config(json_str: &str) -> Config {
    serde_json::from_str(json_str).unwrap()
}

#[test]
fn explicit_self_improvement_provider_wins_as_is() {
    // The override is trusted without a health check; the #469 runtime
    // fallback demotes it if creation fails.
    let cfg = config(r#"{ "agent": { "self_improvement_provider": "minimax" } }"#);
    assert_eq!(resolve_rsi_provider(&cfg), "minimax");
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
    assert_eq!(resolve_rsi_provider(&cfg), "claude_cli");
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
    assert_eq!(resolve_rsi_provider(&cfg), "xiaomi");
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
    assert_eq!(resolve_rsi_provider(&cfg), "minimax");
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
    assert_eq!(resolve_rsi_provider(&cfg), "anthropic");
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
    assert_eq!(resolve_rsi_provider(&cfg), "xiaomi");
}

#[test]
fn registry_walk_remains_the_last_resort() {
    // Nothing declared, no chain: the old behavior is the floor.
    let cfg = config(r#"{ "providers": { "xiaomi": { "enabled": true, "api_key": "k" } } }"#);
    assert_eq!(resolve_rsi_provider(&cfg), "xiaomi");
}
