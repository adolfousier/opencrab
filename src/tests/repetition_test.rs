//! #740: recovery from a repetitive-tool-call poisoned history.

use crate::brain::agent::service::repetition::{
    is_repetitive_tool_error, prune_repetitive_tool_calls,
};
use crate::brain::provider::{ContentBlock, Message, Role};
use serde_json::json;

fn tool_use(id: &str, name: &str, arg: &str) -> Message {
    Message {
        role: Role::Assistant,
        content: vec![ContentBlock::ToolUse {
            id: id.to_string(),
            name: name.to_string(),
            input: json!({ "command": arg }),
        }],
    }
}

fn tool_result(id: &str) -> Message {
    Message {
        role: Role::User,
        content: vec![ContentBlock::ToolResult {
            tool_use_id: id.to_string(),
            content: "connection refused".to_string(),
            is_error: Some(true),
        }],
    }
}

#[test]
fn detects_provider_repetition_guardrail() {
    assert!(is_repetitive_tool_error(
        "API error (500) [invalid_request_error]: Repetitive tool calls detected in the \
         conversation history. The same tool call with identical name and arguments has been \
         repeated across multiple consecutive rounds."
    ));
    assert!(!is_repetitive_tool_error(
        "API error (500): internal server error"
    ));
}

#[test]
fn collapses_consecutive_identical_rounds_to_one() {
    // 5 identical curl rounds → keep 1 pair, remove the other 4 pairs (8 msgs).
    let mut msgs = vec![Message::user("poll the health endpoint")];
    for i in 0..5 {
        msgs.push(tool_use(
            &format!("c{i}"),
            "bash",
            "curl http://127.0.0.1:18789/health",
        ));
        msgs.push(tool_result(&format!("c{i}")));
    }
    let (pruned, removed) = prune_repetitive_tool_calls(&msgs);
    assert_eq!(removed, 8, "4 duplicate pairs removed");
    // user + one tool_use + one tool_result.
    assert_eq!(pruned.len(), 3);
}

#[test]
fn distinct_calls_are_preserved() {
    let msgs = vec![
        tool_use("a", "bash", "ls"),
        tool_result("a"),
        tool_use("b", "bash", "pwd"),
        tool_result("b"),
    ];
    let (pruned, removed) = prune_repetitive_tool_calls(&msgs);
    assert_eq!(removed, 0, "different args are not duplicates");
    assert_eq!(pruned.len(), 4);
}

#[test]
fn non_consecutive_repeats_are_kept() {
    // Same call twice but with a different round in between — not consecutive,
    // so the provider guardrail wouldn't fire and we don't prune.
    let msgs = vec![
        tool_use("a", "bash", "curl x"),
        tool_result("a"),
        tool_use("b", "bash", "pwd"),
        tool_result("b"),
        tool_use("c", "bash", "curl x"),
        tool_result("c"),
    ];
    let (_, removed) = prune_repetitive_tool_calls(&msgs);
    assert_eq!(removed, 0);
}

#[test]
fn rounds_with_real_text_are_never_collapsed() {
    // If the assistant said something alongside the tool call, keep every round.
    let round_with_text = |id: &str| Message {
        role: Role::Assistant,
        content: vec![
            ContentBlock::Text {
                text: "Let me check again".to_string(),
            },
            ContentBlock::ToolUse {
                id: id.to_string(),
                name: "bash".to_string(),
                input: json!({ "command": "curl x" }),
            },
        ],
    };
    let msgs = vec![
        round_with_text("a"),
        tool_result("a"),
        round_with_text("b"),
        tool_result("b"),
    ];
    let (_, removed) = prune_repetitive_tool_calls(&msgs);
    assert_eq!(removed, 0, "text-bearing rounds are preserved");
}
