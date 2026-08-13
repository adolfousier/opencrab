//! Per-family resolution of the DashScope qwen thinking knobs (#1034) and the
//! `preserve_thinking` host gate (#1033).
//!
//! Each qwen family reads exactly one of `reasoning_effort` / `enable_thinking`.
//! Shipping both is the competing-knob shape; shipping only the inert one leaves
//! thinking at the server default instead of what the user configured.

use crate::brain::provider::qwen::serves_qwen_remotely;
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
fn the_tiered_family_defaults_to_the_recommended_tier() {
    // An unconfigured install should get the vendor's recommended setting for
    // this family rather than whatever the endpoint falls back to.
    let knobs = resolve("qwen3.8-max", None, None);
    assert_eq!(knobs.reasoning_effort.as_deref(), Some("xhigh"));
    assert_eq!(knobs.enable_thinking, None);
}

#[test]
fn a_configured_tier_beats_the_default() {
    let knobs = resolve("qwen3.8-max", Some("low"), None);
    assert_eq!(knobs.reasoning_effort.as_deref(), Some("low"));
}

#[test]
fn the_default_tier_does_not_leak_to_hybrids() {
    // Hybrids read the on/off switch; a tier is opaque there, so defaulting
    // one in would put an inert field on every request.
    let knobs = resolve("qwen3.7-plus", None, None);
    assert_eq!(knobs.reasoning_effort, None);
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
fn preserve_thinking_reaches_every_remote_qwen_host() {
    // Not just Alibaba: the same models are served by ModelScope, NVIDIA NIM
    // and other OpenAI-compatible gateways, and an Alibaba-only allowlist
    // left all of them unconfigured (#1040).
    for (url, model) in [
        (
            "https://dashscope.aliyuncs.com/compatible-mode/v1",
            "qwen3.8-max",
        ),
        (
            "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
            "qwen3.7-plus",
        ),
        (
            "https://api-inference.modelscope.ai/v1",
            "Qwen-Ambassador/Qwen3.8-Max",
        ),
        (
            "https://integrate.api.nvidia.com/v1",
            "Qwen/Qwen3.5-397B-A17B",
        ),
        ("https://openrouter.ai/api/v1", "qwen/qwen3.8-max"),
    ] {
        assert!(
            preserve_thinking_for(url, model),
            "{model} on {url} is a remote qwen target"
        );
        assert!(serves_qwen_remotely(url, model));
    }
}

#[test]
fn a_locally_served_qwen_is_still_excluded() {
    // llama.cpp / LM Studio take thinking through `chat_template_kwargs`, so
    // these fields would be a second, inert mechanism there.
    for url in [
        "http://localhost:1234/v1",
        "http://127.0.0.1:8080/v1",
        "http://[::1]:8080/v1",
    ] {
        assert!(!preserve_thinking_for(url, "qwen3.8-max"), "{url} is local");
        assert!(!serves_qwen_remotely(url, "qwen3.8-max"));
    }
}

#[test]
fn a_non_qwen_model_on_a_remote_host_is_untouched() {
    assert!(!preserve_thinking_for("https://api.openai.com/v1", "gpt-5"));
    assert!(!serves_qwen_remotely(
        "https://integrate.api.nvidia.com/v1",
        "deepseek-ai/deepseek-v4-pro"
    ));
}

#[test]
fn a_vendor_prefixed_tiered_model_is_not_mistaken_for_a_hybrid() {
    // The silent misclassification: the raw id fails the tiered prefix but
    // still starts with `qwen`, so it fell through to the hybrid arm and got
    // the switch it does not read while the effort tier went unset (#1040).
    for model in [
        "Qwen-Ambassador/Qwen3.8-Max",
        "qwen/qwen3.8-max-preview",
        "ModelScope/Qwen3.8-Max",
        "SOME-VENDOR/QWEN3.8-MAX",
    ] {
        assert_eq!(
            family(model),
            Some(QwenFamily::TieredEffort),
            "{model} is a tiered-effort model whatever the namespace or case"
        );
    }
}

#[test]
fn a_vendor_prefixed_hybrid_still_classifies_as_a_hybrid() {
    for model in [
        "Qwen/Qwen3.5-397B-A17B",
        "Qwen-Ambassador/Qwen3.7-Plus",
        "ModelScope/Qwen3.6-Flash",
    ] {
        assert_eq!(family(model), Some(QwenFamily::HybridThinking), "{model}");
    }
}

#[test]
fn a_namespaced_non_qwen_model_is_not_classified() {
    // The vendor segment must not decide this either way.
    for model in [
        "deepseek-ai/DeepSeek-V4-Pro",
        "zai-org/GLM-5.2",
        "Qwen-Ambassador/GLM-5.1",
    ] {
        assert_eq!(family(model), None, "{model} is not a qwen model");
    }
}

#[test]
fn a_vendor_prefixed_tiered_model_gets_the_effort_tier() {
    // End to end through resolve: the point of the classification fix.
    let knobs = resolve("Qwen-Ambassador/Qwen3.8-Max", None, Some(true));
    assert_eq!(knobs.reasoning_effort.as_deref(), Some("xhigh"));
    assert_eq!(knobs.enable_thinking, None);
}
