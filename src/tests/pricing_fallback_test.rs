//! Pricing fallback ladder (#655).
//!
//! A model not in the pricing table must not silently cost $0 when its family
//! is recognizable. `estimate_cost` / `calculate_cost_with_cache` resolve via a
//! ladder: exact substring, then a version-agnostic tier match so a new release
//! inherits the last-known price of its tier, then a family-root flagship
//! fallback. Only a genuinely unrecognizable family returns None / $0.

use crate::usage::pricing::{PricingConfig, PricingEntry, ProviderBlock};
use std::collections::HashMap;

const TOK: i64 = 1_000_000;

fn entry(prefix: &str, input: f64, output: f64) -> PricingEntry {
    PricingEntry {
        prefix: prefix.to_string(),
        input_per_m: input,
        output_per_m: output,
        cache_write_per_m: None,
        cache_read_per_m: None,
    }
}

fn cfg() -> PricingConfig {
    let mut providers = HashMap::new();
    providers.insert(
        "anthropic".to_string(),
        ProviderBlock {
            entries: vec![
                entry("claude-opus-4", 5.0, 25.0),
                entry("claude-sonnet-4", 3.0, 15.0),
                entry("claude-haiku-4", 1.0, 5.0),
            ],
        },
    );
    providers.insert(
        "openai".to_string(),
        ProviderBlock {
            entries: vec![entry("gpt-5", 1.25, 10.0)],
        },
    );
    providers.insert(
        "qwen".to_string(),
        ProviderBlock {
            entries: vec![
                entry("qwen3.6-max-preview", 1.30, 7.80),
                entry("qwen-max-preview", 1.30, 7.80),
                entry("qwen-plus", 0.50, 3.0),
            ],
        },
    );
    providers.insert(
        "minimax".to_string(),
        ProviderBlock {
            entries: vec![
                entry("minimax-m3", 0.60, 2.40),
                entry("minimax", 0.30, 1.20),
            ],
        },
    );
    PricingConfig { providers }
}

#[test]
fn exact_match_is_unchanged() {
    let c = cfg();
    // A known model still resolves to its own rate.
    let cost = c.estimate_cost("qwen3.6-max-preview", TOK).unwrap();
    let expected = (TOK as f64 * 0.80 / 1e6) * 1.30 + (TOK as f64 * 0.20 / 1e6) * 7.80;
    assert!((cost - expected).abs() < 1e-9);
}

#[test]
fn new_version_inherits_last_known_tier_not_zero() {
    let c = cfg();
    // qwen3.8-max-preview is absent: fall back to the max-preview tier, not $0.
    let new = c.estimate_cost("qwen3.8-max-preview", TOK);
    assert_eq!(new, c.estimate_cost("qwen-max-preview", TOK));
    assert!(new.unwrap() > 0.0);
    // ...and not the cheaper plus tier.
    assert_ne!(new, c.estimate_cost("qwen-plus", TOK));
}

#[test]
fn new_version_preserves_tier_distinction() {
    let c = cfg();
    assert_eq!(
        c.estimate_cost("qwen3.9-plus", TOK),
        c.estimate_cost("qwen-plus", TOK)
    );
    assert_ne!(
        c.estimate_cost("qwen3.9-plus", TOK),
        c.estimate_cost("qwen-max-preview", TOK)
    );
}

#[test]
fn new_opus_and_sonnet_inherit_their_own_tier() {
    let c = cfg();
    assert_eq!(
        c.estimate_cost("claude-opus-4-9", TOK),
        c.estimate_cost("claude-opus-4", TOK)
    );
    assert_eq!(
        c.estimate_cost("claude-sonnet-9-9", TOK),
        c.estimate_cost("claude-sonnet-4", TOK)
    );
    // opus tier is not sonnet tier.
    assert_ne!(
        c.estimate_cost("claude-opus-4-9", TOK),
        c.estimate_cost("claude-sonnet-4", TOK)
    );
}

#[test]
fn normalized_short_name_resolves_to_family() {
    let c = cfg();
    // "opus-4-9" dropped the vendor prefix; still resolves to the opus tier.
    assert_eq!(
        c.estimate_cost("opus-4-9", TOK),
        c.estimate_cost("claude-opus-4", TOK)
    );
}

#[test]
fn new_gpt_version_inherits_gpt_rate() {
    let c = cfg();
    assert_eq!(
        c.estimate_cost("gpt-5.3", TOK),
        c.estimate_cost("gpt-5", TOK)
    );
    assert!(c.estimate_cost("gpt-5.3", TOK).unwrap() > 0.0);
}

#[test]
fn family_root_fallback_for_a_brand_new_tier() {
    let c = cfg();
    // "qwen-ultra-9" is a tier with no version-agnostic match, but the qwen
    // family flagship (most expensive = max-preview) prices it, never $0.
    let cost = c.estimate_cost("qwen-ultra-9", TOK);
    assert!(cost.is_some());
    assert_eq!(cost, c.estimate_cost("qwen-max-preview", TOK));
}

#[test]
fn unrecognizable_family_still_returns_none_and_zero() {
    let c = cfg();
    assert!(c.estimate_cost("frobnicator-9", TOK).is_none());
    assert_eq!(
        c.calculate_cost_with_cache("frobnicator-9", 1_000_000, 1_000_000, 0, 0),
        0.0
    );
}

#[test]
fn calculate_cost_with_cache_nonzero_for_new_version() {
    let c = cfg();
    let new = c.calculate_cost_with_cache("qwen3.8-max-preview", 1_000_000, 1_000_000, 0, 0);
    let known = c.calculate_cost_with_cache("qwen-max-preview", 1_000_000, 1_000_000, 0, 0);
    assert!(new > 0.0);
    assert!((new - known).abs() < 1e-9);
}
