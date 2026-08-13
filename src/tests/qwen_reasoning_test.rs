//! Per-family resolution of the DashScope qwen thinking knobs (#1034) and the
//! `preserve_thinking` host gate (#1033).
//!
//! Each qwen family reads exactly one of `reasoning_effort` / `enable_thinking`.
//! Shipping both is the competing-knob shape; shipping only the inert one leaves
//! thinking at the server default instead of what the user configured.

use crate::brain::provider::qwen::is_dashscope_host;
use crate::brain::provider::qwen_reasoning::{QwenFamily, family, preserve_thinking_for, resolve};

#[test]
fn the_tiered_family_is_matched_by_prefix() {
    assert_eq!(family("qwen3.8-max"), Some(QwenFamily::TieredEffort));
    assert_eq!(
        family("qwen3.8-max-preview"),
        Some(QwenFamily::TieredEffort)
    );
    assert_eq!(
        family("QWEN3.8-MAX-PREVIEW"),
        Some(QwenFamily::TieredEffort)
    );
}

#[test]
fn older_qwen_models_are_hybrids() {
    for model in [
        "qwen3.6-plus",
        "qwen3.6-flash",
        "qwen3.7-plus",
        "qwen3.7-max",
        "qwen3-coder-plus",
        "qwen-vl-max",
    ] {
        assert_eq!(
            family(model),
            Some(QwenFamily::HybridThinking),
            "{model} should read the on/off switch"
        );
    }
}

#[test]
fn non_qwen_models_are_not_classified() {
    for model in ["glm-5.1", "deepseek-v4-pro", "kimi-k3", "gpt-5"] {
        assert_eq!(family(model), None, "{model} is not a qwen model");
    }
}

#[test]
fn the_tiered_family_ships_the_effort_tier_alone() {
    // `enable_thinking = true` is configured but inert here, and alongside a
    // tier it is a second competing knob, so it must not reach the wire.
    let knobs = resolve("qwen3.8-max-preview", Some("xhigh"), Some(true));
    assert_eq!(knobs.reasoning_effort.as_deref(), Some("xhigh"));
    assert_eq!(knobs.enable_thinking, None);
}

#[test]
fn an_explicit_disable_is_translated_for_the_tiered_family() {
    // Dropping the switch outright would silently turn the user's "off" into
    // "server default"; the family's canonical disable is the `none` tier.
    let knobs = resolve("qwen3.8-max", None, Some(false));
    assert_eq!(knobs.reasoning_effort.as_deref(), Some("none"));
    assert_eq!(knobs.enable_thinking, None);
}

#[test]
fn the_tiered_family_sends_nothing_when_unconfigured() {
    // Thinking is mandatory on this family, so an absent tier still thinks.
    // Inventing a default tier here would override the model's own choice.
    let knobs = resolve("qwen3.8-max", None, None);
    assert_eq!(knobs.reasoning_effort, None);
    assert_eq!(knobs.enable_thinking, None);
}

#[test]
fn hybrids_default_to_thinking_on() {
    let knobs = resolve("qwen3.7-plus", None, None);
    assert_eq!(knobs.enable_thinking, Some(true));
    assert_eq!(knobs.reasoning_effort, None);
}

#[test]
fn hybrids_honour_an_explicit_disable() {
    let knobs = resolve("qwen3.6-plus", None, Some(false));
    assert_eq!(knobs.enable_thinking, Some(false));
}

#[test]
fn a_configured_effort_still_passes_through_on_hybrids() {
    // #691: the effort is opaque to these models rather than a competing knob,
    // and dropping it would put back the bug where a configured tier never
    // reached Model Studio at all.
    let knobs = resolve("qwen3.7-max", Some("xhigh"), Some(true));
    assert_eq!(knobs.reasoning_effort.as_deref(), Some("xhigh"));
    assert_eq!(knobs.enable_thinking, Some(true));
}

#[test]
fn non_qwen_models_get_no_knobs() {
    let knobs = resolve("glm-5.1", Some("high"), Some(true));
    assert_eq!(knobs.reasoning_effort, None);
    assert_eq!(knobs.enable_thinking, None);
}

#[test]
fn preserve_thinking_follows_the_host_not_the_model() {
    for url in [
        "https://dashscope.aliyuncs.com/compatible-mode/v1",
        "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
        "https://dashscope-us.aliyuncs.com/compatible-mode/v1",
        "https://bailian.console.aliyun.com/v1",
    ] {
        assert!(preserve_thinking_for(url), "{url} is a DashScope endpoint");
        assert!(is_dashscope_host(url));
    }
}

#[test]
fn a_locally_served_qwen_gets_no_dashscope_fields() {
    // Matches on model name but not on host: llama.cpp / LM Studio take
    // thinking through `chat_template_kwargs`, not DashScope request fields.
    for url in [
        "http://localhost:1234/v1",
        "http://127.0.0.1:8080/v1",
        "https://api.openai.com/v1",
    ] {
        assert!(!preserve_thinking_for(url), "{url} is not DashScope");
        assert!(!is_dashscope_host(url));
    }
}
