//! Cached brain prefix + uncached runtime suffix (#658).
//!
//! The stable brain must be byte-stable across sessions so the provider prompt
//! cache reuses it; the per-session Runtime Info block rides uncached after the
//! cache breakpoint. These lock in the split (`split_runtime_suffix` /
//! `with_system` / `system_full`) and the qwen request encoding (2-part system
//! with cache_control on the stable part only).

use crate::brain::prompt_builder::split_runtime_suffix;
use crate::brain::provider::custom_openai_compatible::OpenAIProvider;
use crate::brain::provider::qwen::qwen_body_transform;
use crate::brain::provider::{LLMRequest, Message};

const BRAIN: &str = "--- SOUL.md ---\nbe kind\n\n--- Runtime Info ---\n\
     Model: qwen3.8-max-preview\nProvider: modelstudio\nCurrent date: 2026-07-21 (UTC)\n\n\
     --- AGENTS.md ---\nrules here\n";

// ── split_runtime_suffix ────────────────────────────────────────────

#[test]
fn split_extracts_runtime_block_and_leaves_stable_prefix() {
    let (stable, suffix) = split_runtime_suffix(BRAIN);
    assert!(!stable.contains("--- Runtime Info ---"));
    assert!(!stable.contains("Current date"));
    assert!(stable.contains("SOUL.md") && stable.contains("AGENTS.md"));
    let suffix = suffix.expect("runtime block present");
    assert!(suffix.starts_with("--- Runtime Info ---"));
    assert!(suffix.contains("Model: qwen3.8-max-preview") && suffix.contains("Current date"));
}

#[test]
fn split_is_noop_without_runtime_block() {
    // No Runtime Info block -> input returned verbatim (a true no-op).
    let (stable, suffix) = split_runtime_suffix("--- SOUL.md ---\njust soul\n");
    assert_eq!(stable, "--- SOUL.md ---\njust soul\n");
    assert!(suffix.is_none());
}

// ── with_system / system_full ───────────────────────────────────────

#[test]
fn with_system_splits_and_system_full_reconstitutes_at_end() {
    let req = LLMRequest::new("m", vec![]).with_system(BRAIN);
    assert!(!req.system.as_ref().unwrap().contains("Runtime Info"));
    assert!(req.system_suffix.as_ref().unwrap().contains("Current date"));

    let full = req.system_full().unwrap();
    // Runtime Info is now AFTER SOUL and AGENTS (option a).
    let soul = full.find("SOUL").unwrap();
    let agents = full.find("AGENTS").unwrap();
    let runtime = full.find("Runtime Info").unwrap();
    assert!(
        soul < agents && agents < runtime,
        "runtime info must be re-appended last"
    );
}

#[test]
fn system_full_handles_absent_parts() {
    assert!(LLMRequest::new("m", vec![]).system_full().is_none());
    let only_system = LLMRequest::new("m", vec![]).with_system("no marker here");
    assert_eq!(only_system.system_full().as_deref(), Some("no marker here"));
}

// ── qwen request encoding ───────────────────────────────────────────

fn qwen_provider() -> OpenAIProvider {
    OpenAIProvider::with_base_url(
        "k".to_string(),
        "https://ws-x.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1/chat/completions"
            .to_string(),
    )
    .with_name("modelstudio")
}

#[test]
fn qwen_request_encodes_split_system_as_two_parts() {
    let req = LLMRequest::new("qwen3.8-max-preview", vec![Message::user("hi")]).with_system(BRAIN);
    let encoded = qwen_provider().to_openai_request(req);
    let val = serde_json::to_value(&encoded).unwrap();
    let sys = val["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["role"] == "system")
        .expect("system message");
    let parts = sys["content"]
        .as_array()
        .expect("qwen system content is a 2-part array");
    assert_eq!(parts.len(), 2);
    assert!(parts[0]["text"].as_str().unwrap().contains("SOUL.md")); // stable
    assert!(parts[1]["text"].as_str().unwrap().contains("Runtime Info")); // suffix
}

#[test]
fn qwen_body_transform_caches_the_stable_part_not_the_suffix() {
    let body = serde_json::json!({
        "model": "qwen3.8-max-preview",
        "stream": true,
        "messages": [
            { "role": "system", "content": [
                { "type": "text", "text": "stable brain" },
                { "type": "text", "text": "--- Runtime Info ---\nModel: x" }
            ]},
            { "role": "user", "content": "hi" }
        ]
    });
    let out = qwen_body_transform(body);
    let sys = &out["messages"][0];
    assert_eq!(
        sys["content"][0]["cache_control"]["type"], "ephemeral",
        "stable part must carry the cache breakpoint"
    );
    assert!(
        sys["content"][1].get("cache_control").is_none()
            || sys["content"][1]["cache_control"].is_null(),
        "the runtime suffix part must stay uncached"
    );
}
