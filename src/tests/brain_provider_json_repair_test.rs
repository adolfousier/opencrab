use crate::brain::provider::json_repair::*;

#[test]
fn passes_through_valid_json() {
    let v = parse_or_repair(r#"{"a":1,"b":"x"}"#);
    assert_eq!(v["a"], 1);
    assert_eq!(v["b"], "x");
}

#[test]
fn empty_returns_object() {
    let v = parse_or_repair("");
    assert!(v.is_object());
}

#[test]
fn closes_open_string() {
    let v = parse_or_repair(r#"{"command":"git status"#);
    assert_eq!(v["command"], "git status");
}

#[test]
fn closes_missing_brace() {
    let v = parse_or_repair(r#"{"a":1,"b":2"#);
    assert_eq!(v["a"], 1);
    assert_eq!(v["b"], 2);
}

#[test]
fn drops_trailing_key_without_value() {
    let v = parse_or_repair(r#"{"a":1,"b":"#);
    assert_eq!(v["a"], 1);
    assert!(v.get("b").is_none());
}

#[test]
fn closes_nested_array() {
    let v = parse_or_repair(r#"{"items":[1,2,3"#);
    assert_eq!(v["items"][0], 1);
    assert_eq!(v["items"][2], 3);
}

#[test]
fn closes_string_inside_array() {
    let v = parse_or_repair(r#"{"items":["a","b"#);
    assert_eq!(v["items"][0], "a");
    assert_eq!(v["items"][1], "b");
}

#[test]
fn unrecoverable_returns_partial_envelope() {
    let v = parse_or_repair(r#"this is not json"#);
    assert!(v["_repair_failed"].as_bool().unwrap_or(false));
    assert_eq!(v["_partial"], "this is not json");
}

#[test]
fn handles_escaped_quote_in_string() {
    let v = parse_or_repair(r#"{"msg":"he said \"hi"#);
    assert_eq!(v["msg"], "he said \"hi");
}

#[test]
fn strips_trailing_comma() {
    let v = parse_or_repair(r#"{"a":1,"#);
    assert_eq!(v["a"], 1);
}

// -- Tool-text leak detection (#1405) --------------------------------
//
// A model with weak function-calling support "invokes" a tool by dumping the
// call as raw JSON text. The rescue layer converts known shapes; what survives
// is unparseable or unknown-shape residue that used to ride to the user as the
// final answer, and a generic warning block that then fossilised into every
// later compaction summary.
//
// The detector replaced a keyword heuristic (contains "function" && contains
// "arguments") that fired on prose ABOUT tool calls. These pin both halves:
// genuine leaks are caught, and the shapes that used to false-positive are not.

use crate::brain::provider::types::ContentBlock;

#[test]
fn each_call_shape_is_detected_as_a_leak() {
    for text in [
        r#"{"name": "bash", "arguments": {"command": "ls"}}"#,
        r#"{"command": "ls -la"}"#,
        r#"{"function": {"name": "bash", "arguments": "{}"}}"#,
    ] {
        assert!(
            !find_call_shaped_json_spans(text).is_empty(),
            "should be a leak: {text}"
        );
    }
}

/// The regression the detector exists for: models routinely *explain* tool-call
/// JSON in fenced examples. Those are display artifacts, not invocations.
#[test]
fn fenced_json_is_never_a_leak() {
    let fence = "```";
    let text = format!(
        "Here is how a call looks:\n{fence}json\n{{\"name\": \"bash\", \"arguments\": {{\"command\": \"ls\"}}}}\n{fence}\nThat is the shape."
    );
    assert!(find_call_shaped_json_spans(&text).is_empty());
    let (cleaned, stripped) = strip_call_shaped_json(&text);
    assert!(!stripped, "fenced example must survive untouched");
    assert_eq!(cleaned, text);
}

/// What the old keyword heuristic got wrong: prose mentioning the words with
/// no parseable object is not a leak.
#[test]
fn prose_about_tool_calls_is_not_a_leak() {
    for text in [
        "The function takes arguments and returns a result.",
        "Set arguments on the function field, not as text.",
        "I would call {name: bash, arguments: broken",
    ] {
        assert!(
            find_call_shaped_json_spans(text).is_empty(),
            "prose must not trip the detector: {text}"
        );
    }
}

/// Parseable JSON that is not call-shaped is ordinary content.
#[test]
fn non_call_shaped_json_is_left_alone() {
    for text in [
        r#"{"result": 42, "ok": true}"#,
        r#"{"name": "opencrabs"}"#,
        r#"{"arguments": ["a", "b"]}"#,
    ] {
        assert!(
            find_call_shaped_json_spans(text).is_empty(),
            "not call-shaped: {text}"
        );
    }
}

/// The bare-command shape matches on the key alone, so an unfenced object
/// carrying a `command` field is treated as an invocation whatever its intent.
/// Pinned deliberately: it is the widest of the three shapes, and the fence
/// exclusion is what keeps it from firing on the explain-in-a-code-block case.
#[test]
fn the_bare_command_shape_matches_on_the_key_alone() {
    assert!(!find_call_shaped_json_spans(r#"{"command": "anything"}"#).is_empty());
    let fence = "```";
    let fenced = format!("{fence}\n{{\"command\": \"anything\"}}\n{fence}");
    assert!(
        find_call_shaped_json_spans(&fenced).is_empty(),
        "fencing is the escape hatch for this shape"
    );
}

#[test]
fn stripping_removes_the_leak_and_keeps_the_prose() {
    let text = "Let me check.\n{\"name\": \"bash\", \"arguments\": {\"command\": \"ls\"}}\nDone.";
    let (cleaned, stripped) = strip_call_shaped_json(text);
    assert!(stripped);
    assert!(!cleaned.contains("arguments"), "leak survived: {cleaned}");
    assert!(cleaned.contains("Let me check."));
    assert!(cleaned.contains("Done."));
}

#[test]
fn stripping_clean_text_reports_no_leak() {
    let (cleaned, stripped) = strip_call_shaped_json("just an answer");
    assert!(!stripped);
    assert_eq!(cleaned, "just an answer");
}

/// A response that made real structured calls is never a leak, even when its
/// text also contains call-shaped JSON: the calls proved the model can invoke
/// tools properly, so the text is commentary.
#[test]
fn structured_tool_use_blocks_suppress_the_leak_verdict() {
    let blocks = vec![
        ContentBlock::Text {
            text: r#"{"command": "ls"}"#.to_string(),
        },
        ContentBlock::ToolUse {
            id: "1".to_string(),
            name: "bash".to_string(),
            input: serde_json::json!({"command": "ls"}),
        },
    ];
    assert!(!content_has_unrecovered_tool_text(&blocks));
}

#[test]
fn text_only_content_with_call_shaped_json_is_a_leak() {
    let blocks = vec![ContentBlock::Text {
        text: r#"{"name": "bash", "arguments": {"command": "ls"}}"#.to_string(),
    }];
    assert!(content_has_unrecovered_tool_text(&blocks));
}

#[test]
fn ordinary_answers_are_not_leaks() {
    let blocks = vec![ContentBlock::Text {
        text: "The build passed.".to_string(),
    }];
    assert!(!content_has_unrecovered_tool_text(&blocks));
}
