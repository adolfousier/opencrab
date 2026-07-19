//! Tests for the offline fixture-driven replay provider (#619).

use crate::brain::provider::{ContentBlock, LLMRequest, Provider, StopReason};
use crate::eval::replay::{ReplayFixture, ReplayProvider};

fn sample_fixture() -> &'static str {
    r#"{
        "model": "kimi-k3",
        "turns": [
            { "text": "Let me read the file.", "tool": { "name": "read_file", "input": {"path": "a.rs"} }, "input_tokens": 100, "output_tokens": 20 },
            { "text": "Now editing.", "tool": { "name": "edit_file", "input": {"path": "a.rs"} } },
            { "text": "Done." }
        ]
    }"#
}

fn empty_request() -> LLMRequest {
    LLMRequest::new("kimi-k3", vec![])
}

#[tokio::test]
async fn replays_turns_in_order() {
    let provider = ReplayProvider::from_json(sample_fixture()).unwrap();

    // Turn 1: text + read_file tool, stops with ToolUse.
    let r1 = provider.complete(empty_request()).await.unwrap();
    assert_eq!(r1.stop_reason, Some(StopReason::ToolUse));
    assert_eq!(r1.usage.input_tokens, 100);
    assert!(
        matches!(&r1.content[0], ContentBlock::Text { text } if text == "Let me read the file.")
    );
    assert!(matches!(&r1.content[1], ContentBlock::ToolUse { name, .. } if name == "read_file"));

    // Turn 2: text + edit_file tool, stops with ToolUse.
    let r2 = provider.complete(empty_request()).await.unwrap();
    assert_eq!(r2.stop_reason, Some(StopReason::ToolUse));
    assert!(matches!(&r2.content[1], ContentBlock::ToolUse { name, .. } if name == "edit_file"));

    // Turn 3: text only, ends the turn.
    let r3 = provider.complete(empty_request()).await.unwrap();
    assert_eq!(r3.stop_reason, Some(StopReason::EndTurn));
    assert!(matches!(&r3.content[0], ContentBlock::Text { text } if text == "Done."));

    assert_eq!(provider.turns_consumed(), 3);
}

#[tokio::test]
async fn past_end_returns_terminal_end_turn() {
    let provider = ReplayProvider::from_json(sample_fixture()).unwrap();
    for _ in 0..3 {
        let _ = provider.complete(empty_request()).await.unwrap();
    }
    // Exhausted: must terminate the loop, never a ToolUse that would recurse.
    let extra = provider.complete(empty_request()).await.unwrap();
    assert_eq!(extra.stop_reason, Some(StopReason::EndTurn));
    assert!(matches!(&extra.content[0], ContentBlock::Text { text } if text.is_empty()));
}

#[tokio::test]
async fn tool_call_id_is_deterministic_when_omitted() {
    let provider = ReplayProvider::from_json(sample_fixture()).unwrap();
    let r1 = provider.complete(empty_request()).await.unwrap();
    // Fixture omitted the id; a stable index-derived id is used.
    assert!(matches!(&r1.content[1], ContentBlock::ToolUse { id, .. } if id == "replay-tool-0"));
}

#[tokio::test]
async fn stream_emits_same_turn_as_complete() {
    use crate::brain::provider::StreamEvent;
    use futures::StreamExt;

    let provider = ReplayProvider::from_json(sample_fixture()).unwrap();
    let mut stream = provider.stream(empty_request()).await.unwrap();
    let mut saw_tool_start = false;
    let mut saw_stop = false;
    while let Some(ev) = stream.next().await {
        match ev.unwrap() {
            StreamEvent::ContentBlockStart {
                content_block: ContentBlock::ToolUse { name, .. },
                ..
            } => {
                assert_eq!(name, "read_file");
                saw_tool_start = true;
            }
            StreamEvent::MessageStop => saw_stop = true,
            _ => {}
        }
    }
    assert!(saw_tool_start, "tool_use block should stream");
    assert!(saw_stop, "stream must terminate with MessageStop");
    assert_eq!(provider.turns_consumed(), 1);
}

#[test]
fn fixture_defaults_model_when_absent() {
    let f = ReplayFixture::from_json(r#"{ "turns": [] }"#).unwrap();
    assert_eq!(f.model, "replay-model");
}
