//! Tests for tool call group stacking in TUI rendering.
//!
//! When 3+ consecutive tool_group messages appear (possibly with thinking-only
//! assistant messages between them), they should be rendered as a single
//! collapsed summary instead of N separate bullet blocks.

use crate::tui::app::{DisplayMessage, ToolCallEntry, ToolCallGroup};
use uuid::Uuid;

/// Helper: create a tool_group DisplayMessage with N tool calls
fn make_tool_group_msg(num_calls: usize) -> DisplayMessage {
    let calls: Vec<ToolCallEntry> = (0..num_calls)
        .map(|i| ToolCallEntry {
            description: format!("Tool call {}", i),
            success: true,
            details: None,
            completed: true,
            tool_input: serde_json::Value::Null,
        })
        .collect();

    DisplayMessage {
        id: Uuid::new_v4(),
        role: "tool_group".to_string(),
        content: format!("{} tool calls", num_calls),
        timestamp: chrono::Utc::now(),
        token_count: None,
        cost: None,
        approval: None,
        approve_menu: None,
        details: None,
        expanded: false,
        tool_group: Some(ToolCallGroup {
            calls,
            expanded: false,
        }),
    }
}

/// Helper: create a thinking-only assistant message (no visible text)
fn make_thinking_only_msg() -> DisplayMessage {
    DisplayMessage {
        id: Uuid::new_v4(),
        role: "assistant".to_string(),
        content: String::new(), // Empty content
        timestamp: chrono::Utc::now(),
        token_count: None,
        cost: None,
        approval: None,
        approve_menu: None,
        details: Some("Thinking about the problem...".to_string()),
        expanded: false,
        tool_group: None,
    }
}

/// Helper: create a regular assistant message with visible text
fn make_assistant_msg(text: &str) -> DisplayMessage {
    DisplayMessage {
        id: Uuid::new_v4(),
        role: "assistant".to_string(),
        content: text.to_string(),
        timestamp: chrono::Utc::now(),
        token_count: None,
        cost: None,
        approval: None,
        approve_menu: None,
        details: None,
        expanded: false,
        tool_group: None,
    }
}

/// Count consecutive tool_group messages starting from idx, skipping thinking-only assistants
fn count_consecutive_groups(messages: &[DisplayMessage], start_idx: usize) -> (usize, usize) {
    let mut count = 0;
    let mut total_calls = 0;
    let mut lookahead = start_idx;

    while lookahead < messages.len() {
        if let Some(ref group) = messages[lookahead].tool_group {
            count += 1;
            total_calls += group.calls.len();
            lookahead += 1;
        } else if messages[lookahead].role == "assistant"
            && messages[lookahead].content.trim().is_empty()
            && messages[lookahead].details.is_some()
        {
            lookahead += 1;
        } else {
            break;
        }
    }

    (count, total_calls)
}

#[test]
fn three_consecutive_groups_are_stacked() {
    let messages = vec![
        make_tool_group_msg(2),
        make_tool_group_msg(3),
        make_tool_group_msg(1),
    ];

    let (count, total_calls) = count_consecutive_groups(&messages, 0);
    assert_eq!(count, 3, "Should detect 3 consecutive tool groups");
    assert_eq!(total_calls, 6, "Should count 6 total tool calls");
}

#[test]
fn two_consecutive_groups_not_stacked() {
    let messages = vec![make_tool_group_msg(2), make_tool_group_msg(3)];

    let (count, _total_calls) = count_consecutive_groups(&messages, 0);
    assert_eq!(count, 2, "Should detect 2 consecutive tool groups");
    // Stacking threshold is 3, so 2 should NOT be stacked (handled in render logic)
}

#[test]
fn groups_with_thinking_between_are_stacked() {
    let messages = vec![
        make_tool_group_msg(2),
        make_thinking_only_msg(),
        make_tool_group_msg(3),
        make_thinking_only_msg(),
        make_tool_group_msg(1),
    ];

    let (count, total_calls) = count_consecutive_groups(&messages, 0);
    assert_eq!(
        count, 3,
        "Should detect 3 tool groups across thinking-only messages"
    );
    assert_eq!(total_calls, 6, "Should count 6 total tool calls");
}

#[test]
fn groups_broken_by_visible_text_not_stacked() {
    let messages = vec![
        make_tool_group_msg(2),
        make_tool_group_msg(3),
        make_assistant_msg("Here's the result"), // Visible text breaks the chain
        make_tool_group_msg(1),
    ];

    let (count, total_calls) = count_consecutive_groups(&messages, 0);
    assert_eq!(
        count, 2,
        "Should only count 2 groups before the visible text"
    );
    assert_eq!(
        total_calls, 5,
        "Should count 5 tool calls in first 2 groups"
    );

    // Check the group after the break
    let (count2, total_calls2) = count_consecutive_groups(&messages, 3);
    assert_eq!(count2, 1, "Should detect 1 group after the break");
    assert_eq!(total_calls2, 1, "Should count 1 tool call");
}

#[test]
fn single_group_not_stacked() {
    let messages = vec![make_tool_group_msg(5)];

    let (count, total_calls) = count_consecutive_groups(&messages, 0);
    assert_eq!(count, 1, "Should detect 1 tool group");
    assert_eq!(total_calls, 5, "Should count 5 tool calls");
}

#[test]
fn empty_messages_not_counted() {
    let messages: Vec<DisplayMessage> = vec![];

    let (count, total_calls) = count_consecutive_groups(&messages, 0);
    assert_eq!(count, 0, "Should detect 0 tool groups in empty list");
    assert_eq!(total_calls, 0, "Should count 0 tool calls");
}
