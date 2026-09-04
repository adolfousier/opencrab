//! z.ai GLM request knobs: `tool_stream` (#1347) and the Preserved Thinking
//! object (#1348). Which requests get them, and what the wire body carries.

use crate::brain::provider::custom_openai_compatible::OpenAIProvider;
use crate::brain::provider::glm_reasoning::{
    GlmThinking, glm_version, serves_zai, thinking_for, tool_stream_for,
};
use crate::brain::provider::{LLMRequest, Message};

const ZAI_API: &str = "https://api.z.ai/api/paas/v4/chat/completions";
const ZAI_CODING: &str = "https://api.z.ai/api/coding/paas/v4/chat/completions";
const BIGMODEL: &str = "https://open.bigmodel.cn/api/paas/v4/chat/completions";
const OPENROUTER: &str = "https://openrouter.ai/api/v1";

fn provider(base_url: &str) -> OpenAIProvider {
    OpenAIProvider::with_base_url("test-key".to_string(), base_url.to_string()).with_name("zhipu")
}

fn body(base_url: &str, model: &str, stream: bool) -> serde_json::Value {
    body_with(provider(base_url), model, stream)
}

fn body_with(provider: OpenAIProvider, model: &str, stream: bool) -> serde_json::Value {
    let mut req = LLMRequest::new(model.to_string(), vec![Message::user("hi".to_string())]);
    req.stream = stream;
    serde_json::to_value(provider.to_openai_request(req)).expect("serialize")
}

#[test]
fn both_documented_zai_hosts_are_recognised() {
    assert!(serves_zai(ZAI_API));
    assert!(serves_zai(ZAI_CODING));
    assert!(serves_zai(BIGMODEL));
}

#[test]
fn a_glm_model_on_another_gateway_is_not_zai() {
    // OpenRouter and Model Studio re-serve glm-* and do not read z.ai fields.
    assert!(!serves_zai(OPENROUTER));
    assert_eq!(tool_stream_for(OPENROUTER, true), None);
}

#[test]
fn tool_stream_rides_only_on_streamed_zai_requests() {
    assert_eq!(tool_stream_for(ZAI_API, true), Some(true));
    assert_eq!(tool_stream_for(BIGMODEL, true), Some(true));
    assert_eq!(
        tool_stream_for(ZAI_API, false),
        None,
        "documented as streaming-only"
    );
}

#[test]
fn the_wire_body_carries_tool_stream_for_zai_and_omits_it_elsewhere() {
    assert_eq!(
        body(ZAI_API, "glm-5.3", true)["tool_stream"],
        serde_json::json!(true)
    );
    assert!(body(ZAI_API, "glm-5.3", false).get("tool_stream").is_none());
    assert!(
        body(OPENROUTER, "z-ai/glm-5.3", true)
            .get("tool_stream")
            .is_none()
    );
}

// ------------------------------------------------------------- #1348

#[test]
fn glm_versions_parse_through_prefix_case_and_suffix() {
    assert_eq!(glm_version("glm-5.3"), Some((5, 3)));
    assert_eq!(glm_version("glm-5.3-flash"), Some((5, 3)));
    assert_eq!(glm_version("z-ai/GLM-5"), Some((5, 0)));
    assert_eq!(glm_version("glm-4.7"), Some((4, 7)));
    assert_eq!(glm_version("qwen3.8-max"), None);
    assert_eq!(glm_version("glm-"), None);
}

#[test]
fn glm_5_on_zai_gets_preserved_thinking() {
    let t = thinking_for(ZAI_API, "glm-5.3", None).expect("glm-5.3 on z.ai");
    assert_eq!(
        t,
        GlmThinking {
            enabled: true,
            off_ignored: false
        }
    );
    assert_eq!(
        t.to_json(),
        serde_json::json!({ "type": "enabled", "clear_thinking": false })
    );
}

#[test]
fn nothing_changes_off_zai_or_below_glm_5() {
    assert_eq!(thinking_for(OPENROUTER, "z-ai/glm-5.3", None), None);
    assert_eq!(thinking_for(ZAI_API, "glm-4.7", None), None);
    assert_eq!(thinking_for(ZAI_API, "some-alias", None), None);
    assert!(body(ZAI_API, "glm-4.7", true).get("thinking").is_none());
}

#[test]
fn thinking_off_is_honoured_before_5_3_and_reported_ignored_after() {
    let older = thinking_for(ZAI_API, "glm-5.1", Some(false)).expect("glm-5.1");
    assert!(!older.enabled);
    assert_eq!(older.to_json(), serde_json::json!({ "type": "disabled" }));

    let newer = thinking_for(ZAI_API, "glm-5.3-flash", Some(false)).expect("glm-5.3-flash");
    assert!(newer.enabled, "5.3+ cannot turn thinking off");
    assert!(newer.off_ignored, "the caller must be told to log it");
}

#[test]
fn the_wire_body_carries_the_thinking_object_for_glm_5_on_zai() {
    let b = body(ZAI_API, "glm-5.3", true);
    assert_eq!(
        b["thinking"],
        serde_json::json!({ "type": "enabled", "clear_thinking": false })
    );
    let off = body_with(provider(ZAI_API).with_enable_thinking(false), "glm-5", true);
    assert_eq!(off["thinking"], serde_json::json!({ "type": "disabled" }));
}
