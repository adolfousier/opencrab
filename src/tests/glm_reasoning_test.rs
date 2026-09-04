//! z.ai GLM request knobs (#1347): which requests get `tool_stream`, and that
//! a fragmented tool-call argument stream still reassembles.

use crate::brain::provider::custom_openai_compatible::OpenAIProvider;
use crate::brain::provider::glm_reasoning::{serves_zai, tool_stream_for};
use crate::brain::provider::{LLMRequest, Message};

const ZAI_API: &str = "https://api.z.ai/api/paas/v4/chat/completions";
const ZAI_CODING: &str = "https://api.z.ai/api/coding/paas/v4/chat/completions";
const BIGMODEL: &str = "https://open.bigmodel.cn/api/paas/v4/chat/completions";
const OPENROUTER: &str = "https://openrouter.ai/api/v1";

fn provider(base_url: &str) -> OpenAIProvider {
    OpenAIProvider::with_base_url("test-key".to_string(), base_url.to_string()).with_name("zhipu")
}

fn body(base_url: &str, model: &str, stream: bool) -> serde_json::Value {
    let mut req = LLMRequest::new(model.to_string(), vec![Message::user("hi".to_string())]);
    req.stream = stream;
    serde_json::to_value(provider(base_url).to_openai_request(req)).expect("serialize")
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
