//! DeepSeek thinking knobs: which requests get them, and what they carry.
//!
//! DeepSeek reads a top-level `thinking` toggle and a `low | high | max`
//! effort. It ignores DashScope's `enable_thinking`, so a DeepSeek target that
//! fell through the qwen path had the knob it reads left unset while a qwen
//! rung it has no equivalent for rode to the wire.

use crate::brain::provider::deepseek_reasoning::{
    default_context_window, resolve, serves_deepseek,
};

const API: &str = "https://api.deepseek.com/v1";
const GATEWAY: &str = "https://openrouter.ai/api/v1";
const LOCAL: &str = "http://localhost:1234/v1";

#[test]
fn the_model_id_is_enough_on_any_gateway() {
    // These models are re-served under namespaced ids, so recognition cannot
    // depend on the endpoint being DeepSeek's own.
    assert!(serves_deepseek(GATEWAY, "deepseek/deepseek-v4-pro"));
    assert!(serves_deepseek(GATEWAY, "deepseek-v4-flash"));
}

#[test]
fn the_endpoint_is_enough_when_the_id_does_not_say_so() {
    assert!(serves_deepseek(API, "some-internal-alias"));
}

#[test]
fn matching_ignores_case_and_vendor_prefix() {
    assert!(serves_deepseek(GATEWAY, "SomeVendor/DeepSeek-V4-Pro"));
    assert!(serves_deepseek(GATEWAY, "DEEPSEEK-V4-FLASH"));
}

#[test]
fn other_families_on_a_shared_gateway_are_untouched() {
    // Model Studio and OpenRouter serve qwen and glm beside DeepSeek. Handing
    // them these knobs would overwrite the ones they do read.
    assert!(!serves_deepseek(GATEWAY, "qwen3.8-max"));
    assert!(!serves_deepseek(GATEWAY, "glm-5.1"));
}

#[test]
fn a_locally_served_model_takes_neither_knob() {
    // llama.cpp and MLX take thinking through chat_template_kwargs.
    assert!(!serves_deepseek(LOCAL, "deepseek-v4-flash"));
}

#[test]
fn an_unconfigured_target_gets_the_vendor_default() {
    // The point of the defaults: an entry with nothing set must still think.
    let knobs = resolve(None, None);
    assert!(knobs.enabled);
    assert_eq!(knobs.effort, Some("high"));
}

#[test]
fn each_rung_of_the_ladder_passes_through() {
    for rung in ["low", "high", "max"] {
        let knobs = resolve(Some(rung), None);
        assert!(knobs.enabled);
        assert_eq!(knobs.effort, Some(rung));
    }
}

#[test]
fn a_rung_deepseek_does_not_have_falls_to_the_default() {
    // `xhigh` is qwen3.8-max's top rung. Sent here it is rejected outright,
    // so a value the user could legitimately have set on another provider
    // must not brick every request.
    let knobs = resolve(Some("xhigh"), None);
    assert!(knobs.enabled);
    assert_eq!(knobs.effort, Some("high"));
}

#[test]
fn an_explicit_off_disables_thinking_and_sends_no_effort() {
    // Off is not a rung: it is the toggle. Sending an effort beside a disabled
    // toggle would be a second competing knob.
    for spelling in ["off", "none", "disabled", "OFF"] {
        let knobs = resolve(Some(spelling), None);
        assert!(!knobs.enabled, "{spelling} must disable thinking");
        assert_eq!(knobs.effort, None, "{spelling} must send no effort");
    }
}

#[test]
fn the_dashscope_switch_still_means_off() {
    // A user who set `enable_thinking = false` meant thinking off, whichever
    // vendor's spelling they reached for.
    let knobs = resolve(None, Some(false));
    assert!(!knobs.enabled);
    assert_eq!(knobs.effort, None);
}

#[test]
fn the_published_window_is_used_only_for_deepseek_models() {
    assert_eq!(default_context_window("deepseek-v4-pro"), Some(1_000_000));
    assert_eq!(
        default_context_window("deepseek/deepseek-v4-flash"),
        Some(1_000_000)
    );
    assert_eq!(default_context_window("qwen3.8-max"), None);
}

// ── the shapes DeepSeek expects on the wire ───────────────────────────

use crate::brain::provider::custom_openai_compatible::OpenAIMessage;

/// The body as it would be serialized, for one assistant message.
fn serialize(msg: &OpenAIMessage) -> serde_json::Value {
    serde_json::to_value(msg).expect("assistant message serializes")
}

#[test]
fn a_tool_call_only_turn_carries_an_empty_content_string() {
    // Not null and not absent. DeepSeek's own harness replays "" here and
    // records that some gateways reject null; dropping the field is a third
    // state neither of them describes.
    let msg = OpenAIMessage {
        role: "assistant".to_string(),
        content: Some(serde_json::Value::String(String::new())),
        tool_calls: None,
        tool_call_id: None,
        reasoning_content: None,
    };
    let body = serialize(&msg);
    assert_eq!(
        body.get("content"),
        Some(&serde_json::Value::String(String::new())),
        "the field must be present and empty"
    );
}

#[test]
fn an_absent_content_is_dropped_from_the_body_entirely() {
    // Pins why the fix was needed: None does not serialize as null, it
    // vanishes, so the model never sees the field at all.
    let msg = OpenAIMessage {
        role: "assistant".to_string(),
        content: None,
        tool_calls: None,
        tool_call_id: None,
        reasoning_content: None,
    };
    assert!(
        serialize(&msg).get("content").is_none(),
        "None omits the key rather than emitting null"
    );
}
