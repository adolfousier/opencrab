//! Tests for Kimi coding-plan tier -> context-window derivation (#612).

use crate::brain::provider::kimi_plan::{PLAN_TIERS, context_window_for_plan};

#[test]
fn moderato_maps_to_256k() {
    assert_eq!(
        context_window_for_plan("moderato", Some("k3")),
        Some(256_000)
    );
}

#[test]
fn allegretto_and_above_map_to_1m_on_k3() {
    for tier in ["allegretto", "allegro", "vivace"] {
        assert_eq!(
            context_window_for_plan(tier, Some("k3")),
            Some(1_000_000),
            "tier {tier} should give 1M on k3"
        );
    }
}

#[test]
fn k27_coding_models_cap_at_256k_on_every_tier() {
    for tier in PLAN_TIERS {
        for model in ["kimi-for-coding", "kimi-for-coding-highspeed"] {
            assert_eq!(
                context_window_for_plan(tier, Some(model)),
                Some(256_000),
                "model {model} on tier {tier} must cap at 256K"
            );
        }
    }
}

#[test]
fn unknown_tier_returns_none() {
    assert_eq!(context_window_for_plan("presto", Some("k3")), None);
    assert_eq!(context_window_for_plan("", Some("k3")), None);
}

#[test]
fn plan_matching_is_case_and_whitespace_insensitive() {
    assert_eq!(
        context_window_for_plan("  Allegretto  ", Some("k3")),
        Some(1_000_000)
    );
}

#[test]
fn tier_list_is_ordered_cheapest_first() {
    assert_eq!(PLAN_TIERS, ["moderato", "allegretto", "allegro", "vivace"]);
}
