use crate::brain::provider::qwen::*;

#[test]
fn extra_headers_match_qwen_cli_exactly() {
    let h = qwen_extra_headers();
    let names: Vec<&str> = h.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(h.len(), 4, "expected exactly 4 headers, got {:?}", names);
    assert!(names.contains(&"User-Agent"));
    assert!(names.contains(&"X-DashScope-CacheControl"));
    assert!(names.contains(&"X-DashScope-UserAgent"));
    assert!(names.contains(&"X-DashScope-AuthType"));
    let ua = h
        .iter()
        .find(|(k, _)| k == "User-Agent")
        .map(|(_, v)| v.clone())
        .unwrap();
    let ds_ua = h
        .iter()
        .find(|(k, _)| k == "X-DashScope-UserAgent")
        .map(|(_, v)| v.clone())
        .unwrap();
    assert_eq!(ua, ds_ua);
    assert!(ua.starts_with("QwenCode/"));
    let auth = h
        .iter()
        .find(|(k, _)| k == "X-DashScope-AuthType")
        .map(|(_, v)| v.clone())
        .unwrap();
    assert_eq!(auth, "qwen-oauth");
}

fn sample_body() -> serde_json::Value {
    serde_json::json!({
        "model": "coder-model",
        "messages": [
            { "role": "system", "content": "sys prompt" },
            { "role": "user", "content": "first user" },
            { "role": "assistant", "content": "asst reply" },
            { "role": "user", "content": "last user" }
        ],
        "temperature": 0.7,
        "top_p": 0.95,
        "tool_choice": "auto",
        "max_completion_tokens": 8192,
        "include_reasoning": true,
        "stream": true,
        "stream_options": { "include_usage": true },
        "tools": [
            {
                "type": "function",
                "function": { "name": "first_tool", "description": "", "parameters": {} }
            },
            {
                "type": "function",
                "function": { "name": "last_tool", "description": "", "parameters": {} }
            }
        ]
    })
}

#[test]
fn body_transform_cache_control_streaming_system_and_last_message() {
    let out = qwen_body_transform(sample_body());
    let msgs = out.get("messages").and_then(|v| v.as_array()).unwrap();

    let sys = &msgs[0];
    assert_eq!(sys["role"], "system");
    assert!(sys["content"].is_array());
    assert_eq!(sys["content"][0]["type"], "text");
    assert_eq!(sys["content"][0]["cache_control"]["type"], "ephemeral");

    assert!(msgs[1]["content"].is_string());
    assert!(msgs[2]["content"].is_string());

    let u2 = &msgs[3];
    assert!(u2["content"].is_array());
    assert_eq!(u2["content"][0]["cache_control"]["type"], "ephemeral");
}

#[test]
fn body_transform_non_streaming_only_tags_system() {
    let mut body = sample_body();
    body["stream"] = serde_json::json!(false);
    let out = qwen_body_transform(body);
    let msgs = out.get("messages").and_then(|v| v.as_array()).unwrap();

    assert!(msgs[0]["content"].is_array());
    assert_eq!(msgs[0]["content"][0]["cache_control"]["type"], "ephemeral");
    assert!(msgs[3]["content"].is_string());
}

#[test]
fn body_transform_preserves_all_fields() {
    let out = qwen_body_transform(sample_body());
    let obj = out.as_object().unwrap();
    assert_eq!(obj.get("temperature"), Some(&serde_json::json!(0.7)));
    assert_eq!(obj.get("top_p"), Some(&serde_json::json!(0.95)));
    assert_eq!(obj.get("tool_choice"), Some(&serde_json::json!("auto")));
    assert_eq!(
        obj.get("max_completion_tokens"),
        Some(&serde_json::json!(8192))
    );
    assert_eq!(obj.get("include_reasoning"), Some(&serde_json::json!(true)));
}

#[test]
fn body_transform_adds_metadata_with_session_and_prompt_ids() {
    let out = qwen_body_transform(sample_body());
    let meta = out.get("metadata").unwrap();
    assert!(meta["sessionId"].is_string());
    assert!(meta["promptId"].is_string());
}

#[test]
fn body_transform_vl_flag_only_for_vision_models() {
    let out = qwen_body_transform(sample_body());
    assert_eq!(out["vl_high_resolution_images"], true);

    let mut body = sample_body();
    body["model"] = serde_json::json!("qwen3-32b");
    let out = qwen_body_transform(body);
    assert!(
        out.as_object()
            .unwrap()
            .get("vl_high_resolution_images")
            .is_none(),
        "text-only model should not carry vl_high_resolution_images"
    );
}

#[test]
fn body_transform_does_not_force_max_tokens() {
    let mut body = sample_body();
    body.as_object_mut().unwrap().remove("max_tokens");
    let out = qwen_body_transform(body);
    assert!(out.as_object().unwrap().get("max_tokens").is_none());
}

#[test]
fn body_transform_tags_last_tool_only_when_streaming() {
    let out = qwen_body_transform(sample_body());
    let tools = out.get("tools").and_then(|v| v.as_array()).unwrap();
    assert!(tools[0].get("cache_control").is_none());
    assert_eq!(tools[1]["cache_control"]["type"], "ephemeral");

    let mut body = sample_body();
    body["stream"] = serde_json::json!(false);
    let out = qwen_body_transform(body);
    let tools = out.get("tools").and_then(|v| v.as_array()).unwrap();
    assert!(tools[0].get("cache_control").is_none());
    assert!(tools[1].get("cache_control").is_none());
}

#[test]
fn body_transform_preserves_existing_max_tokens() {
    let mut body = sample_body();
    body["max_tokens"] = serde_json::json!(4096);
    let out = qwen_body_transform(body);
    assert_eq!(out["max_tokens"], 4096);
}

#[test]
fn body_transform_cache_control_on_multimodal_last_message() {
    let body = serde_json::json!({
        "model": "coder-model",
        "stream": true,
        "messages": [
            { "role": "system", "content": "sys" },
            {
                "role": "user",
                "content": [
                    { "type": "text", "text": "look at this" },
                    { "type": "image_url", "image_url": { "url": "data:..." } }
                ]
            }
        ]
    });
    let out = qwen_body_transform(body);
    let msgs = out["messages"].as_array().unwrap();
    let u = &msgs[1];
    let content = u["content"].as_array().unwrap();
    assert_eq!(content.len(), 2);
    assert_eq!(content[0]["type"], "text");
    assert_eq!(content[1]["type"], "image_url");
    assert_eq!(content[1]["cache_control"]["type"], "ephemeral");
}

#[test]
fn is_vision_model_matches_qwen_cli_list() {
    assert!(is_vision_model("coder-model"));
    assert!(is_vision_model("qwen-vl-max"));
    assert!(is_vision_model("qwen-vl-max-latest"));
    assert!(is_vision_model("qwen3-vl-plus"));
    assert!(is_vision_model("qwen3.5-plus"));
    assert!(is_vision_model("CODER-MODEL"));
    assert!(!is_vision_model("qwen3-32b"));
    assert!(!is_vision_model("qwen-max"));
    assert!(!is_vision_model(""));
}

#[test]
fn session_id_is_stable_within_process() {
    let a = qwen_session_id();
    let b = qwen_session_id();
    assert_eq!(a, b);
}

#[test]
fn prompt_id_is_13_hex_chars() {
    let id = qwen_prompt_id();
    assert_eq!(id.len(), 13);
    assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
}
