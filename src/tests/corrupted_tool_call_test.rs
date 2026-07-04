//! Tests for corrupted tool-call separator recovery (issue #287).
//!
//! 2026-07-03: mimo-v2.5 (opencode-go) emits tool calls where the tool
//! name and JSON params are valid but the separator between them is
//! corrupted into non-ASCII garbage (Bengali script). The extractor must
//! still detect and dispatch these calls.

use crate::brain::provider::custom_openai_compatible::extract_text_tool_calls;

#[test]
fn corrupted_separator_bengali_dispatches_tool_call() {
    // Exact artifact from session 8c5b869a: "tool_search ব্যক{"query":"read excel file"}"
    let text = "tool_search ব্যক{\"query\": \"read excel file\"}";
    let (calls, cleaned) = extract_text_tool_calls(text);

    assert_eq!(calls.len(), 1, "should extract one tool call");
    assert_eq!(calls[0].0, "tool_search");
    assert_eq!(calls[0].1, serde_json::json!({"query": "read excel file"}));
    // The corrupted block should be stripped from visible text.
    assert!(
        !cleaned.contains("ব্যক"),
        "Bengali garbage should be stripped from cleaned text"
    );
    assert!(
        !cleaned.contains("tool_search"),
        "tool name should be stripped from cleaned text"
    );
}

#[test]
fn corrupted_separator_second_bengali_artifact() {
    let text = "tool_search ব্যক{\"query\": \"analyze excel file content\"}";
    let (calls, _cleaned) = extract_text_tool_calls(text);

    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "tool_search");
    assert_eq!(
        calls[0].1,
        serde_json::json!({"query": "analyze excel file content"})
    );
}

#[test]
fn corrupted_separator_with_surrounding_prose() {
    let text = "Let me search for that. tool_search ব্যক{\"query\": \"find files\"} Done.";
    let (calls, cleaned) = extract_text_tool_calls(text);

    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "tool_search");
    assert!(cleaned.contains("Let me search for that."));
    assert!(cleaned.contains("Done."));
}

#[test]
fn prose_with_ascii_letters_after_tool_name_not_matched() {
    // "tool_search for {reason}" must NOT be swept in — the gap has
    // ASCII letters, so it's prose, not a corrupted separator.
    let text = "I'll use tool_search for this query {reason}";
    let (calls, _cleaned) = extract_text_tool_calls(text);

    assert!(
        calls.is_empty(),
        "prose with ASCII letters between tool name and brace must not match"
    );
}

#[test]
fn plain_json_without_tool_name_not_matched() {
    // A bare JSON object with no tool name before it should not trigger
    // the corrupted-separator pass.
    let text = "Here is some data: {\"key\": \"value\"}";
    let (calls, _cleaned) = extract_text_tool_calls(text);

    assert!(
        calls.is_empty(),
        "plain JSON without tool name must not match"
    );
}

#[test]
fn corrupted_separator_inside_wrapper_not_double_matched() {
    // If the tool call is already inside a <tool_call> wrapper, the
    // corrupted pass must not claim it a second time.
    let text = "<tool_call>tool_search ব্যক{\"query\": \"test\"}</tool_call>";
    let (calls, _cleaned) = extract_text_tool_calls(text);

    assert_eq!(calls.len(), 1, "should extract exactly one call, not two");
    assert_eq!(calls[0].0, "tool_search");
}

#[test]
fn corrupted_separator_multiple_calls_in_one_message() {
    let text = "tool_search ব্যক{\"query\": \"first\"} and tool_search ব্যক{\"query\": \"second\"}";
    let (calls, _cleaned) = extract_text_tool_calls(text);

    assert_eq!(calls.len(), 2, "should extract both calls");
    assert_eq!(calls[0].0, "tool_search");
    assert_eq!(calls[1].0, "tool_search");
    assert_eq!(calls[0].1, serde_json::json!({"query": "first"}));
    assert_eq!(calls[1].1, serde_json::json!({"query": "second"}));
}
