//! Tests for the Qwen-style text-based tool-call extractor.
//!
//! Local GGUF/MLX backends (llama.cpp, MLX, LM Studio, Ollama) serving Qwen3
//! often emit tool calls as `<tool_call>{json}</tool_call>` or
//! `<function=name>...<parameter=key>val</parameter>...</function>` inside
//! `message.content` instead of the structured `tool_calls` field. The
//! extractor recovers them so they execute; these tests lock in the
//! contract — especially the edge cases (nested braces, open-ended tags,
//! prose mentions, field aliases).

use crate::brain::provider::custom_openai_compatible::{
    BareToolArrayMatch, classify_bare_tool_array, extract_balanced_json, extract_text_tool_calls,
};

#[test]
fn balanced_json_simple() {
    assert_eq!(extract_balanced_json(r#"{"a":1}"#), Some(7));
    assert_eq!(extract_balanced_json(r#"{"a":{"b":2}} trailing"#), Some(13));
}

#[test]
fn balanced_json_strings_with_braces() {
    // Braces inside strings must not affect depth.
    let s = r#"{"cmd":"echo { nested } end"} trailing"#;
    let consumed = extract_balanced_json(s).expect("balanced");
    assert_eq!(&s[..consumed], r#"{"cmd":"echo { nested } end"}"#);
}

#[test]
fn balanced_json_escaped_quotes() {
    let s = r#"{"msg":"he said \"hi\" then left"}"#;
    let consumed = extract_balanced_json(s).expect("balanced");
    assert_eq!(consumed, s.len());
}

#[test]
fn balanced_json_unbalanced_returns_none() {
    assert_eq!(extract_balanced_json(r#"{"a":1"#), None);
    assert_eq!(extract_balanced_json("not json"), None);
}

#[test]
fn extract_tool_call_closed() {
    let text = r#"sure, running it. <tool_call>{"name":"bash","arguments":{"command":"ls"}}</tool_call> done."#;
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "bash");
    assert_eq!(calls[0].1["command"], "ls");
    assert!(!cleaned.contains("<tool_call>"));
    assert!(cleaned.contains("sure, running it"));
    assert!(cleaned.contains("done"));
}

#[test]
fn extract_tool_call_open_ended() {
    // Qwen frequently omits the closing tag.
    let text = r#"<tool_call>{"name":"web_search","arguments":{"query":"rust traits"}}"#;
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "web_search");
    assert_eq!(calls[0].1["query"], "rust traits");
    assert!(cleaned.trim().is_empty());
}

#[test]
fn extract_tool_call_nested_braces() {
    // Arguments with nested JSON must survive balanced-brace extraction.
    let text = r#"<tool_call>{"name":"set","arguments":{"obj":{"k":"v"},"n":1}}</tool_call>"#;
    let (calls, _) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "set");
    assert_eq!(calls[0].1["obj"]["k"], "v");
    assert_eq!(calls[0].1["n"], 1);
}

#[test]
fn extract_multiple_tool_calls() {
    let text = concat!(
        "first <tool_call>{\"name\":\"a\",\"arguments\":{}}</tool_call> ",
        "then <tool_call>{\"name\":\"b\",\"arguments\":{\"x\":2}}</tool_call>"
    );
    let (calls, _) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].0, "a");
    assert_eq!(calls[1].0, "b");
    assert_eq!(calls[1].1["x"], 2);
}

#[test]
fn extract_tool_call_with_field_aliases() {
    // MiniMax-style `tool_name` + `args`.
    let text = r#"<tool_call>{"tool_name":"bash","args":{"command":"pwd"}}</tool_call>"#;
    let (calls, _) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "bash");
    assert_eq!(calls[0].1["command"], "pwd");
}

#[test]
fn extract_tool_call_stringified_arguments() {
    // Some runtimes emit arguments as a JSON-encoded string.
    let text = r#"<tool_call>{"name":"run","arguments":"{\"cmd\":\"go\"}"}</tool_call>"#;
    let (calls, _) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "run");
    assert_eq!(calls[0].1["cmd"], "go");
}

#[test]
fn extract_function_format() {
    let text = r#"<function=web_search><parameter=query>rust</parameter></function>"#;
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "web_search");
    assert_eq!(calls[0].1["query"], "rust");
    assert!(cleaned.trim().is_empty());
}

#[test]
fn skips_prose_mention_with_invalid_json() {
    // "the <tool_call> tag is special" — no JSON, so we must not strip it
    // and must not emit a tool call.
    let text = "the <tool_call> tag is special";
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 0);
    assert_eq!(cleaned, text);
}

#[test]
fn noop_without_markers() {
    let text = "just prose, no tool tags here";
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 0);
    assert_eq!(cleaned, text);
}

#[test]
fn ignores_tool_call_without_name() {
    // Malformed — no `name`/`tool_name` field → must not emit a call.
    let text = r#"<tool_call>{"arguments":{"x":1}}</tool_call>"#;
    let (calls, _) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 0);
}

#[test]
fn extract_bare_tool_call_openai_envelope() {
    // The format Qwen3 leaks when the template isn't in reasoning mode —
    // seen in logs 2026-04-17 02:41:12 after phantom retries failed.
    let text = r#"tool_call:{"id":"call_001","type":"function","function":{"name":"bash","arguments":{"command":"ls -la"}}}"#;
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 1, "must recover bare tool_call: prefix");
    assert_eq!(calls[0].0, "bash");
    assert_eq!(calls[0].1["command"], "ls -la");
    assert!(
        cleaned.trim().is_empty(),
        "the whole envelope must be stripped, got: {cleaned:?}"
    );
}

#[test]
fn extract_bare_tool_call_with_preceding_text() {
    let text = r#"I'll check that. tool_call:{"id":"c1","type":"function","function":{"name":"bash","arguments":{"command":"pwd"}}} done."#;
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "bash");
    assert!(cleaned.contains("I'll check that"));
    assert!(cleaned.contains("done"));
    assert!(!cleaned.contains("tool_call:"));
}

#[test]
fn bare_marker_rejects_non_boundary_prefix() {
    // `set_tool_call:{...}` is not a tool-call emission — the prefix is
    // embedded in an identifier. Must not extract.
    let text = r#"set_tool_call:{"x":1} and more"#;
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 0);
    assert_eq!(cleaned, text);
}

#[test]
fn extract_tool_calls_array_envelope() {
    let text = r#"{"tool_calls":[{"id":"c1","type":"function","function":{"name":"bash","arguments":{"command":"ls"}}},{"id":"c2","type":"function","function":{"name":"web_search","arguments":{"query":"rust"}}}]}"#;
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 2, "both envelope entries must be recovered");
    assert_eq!(calls[0].0, "bash");
    assert_eq!(calls[0].1["command"], "ls");
    assert_eq!(calls[1].0, "web_search");
    assert_eq!(calls[1].1["query"], "rust");
    assert!(cleaned.trim().is_empty());
}

#[test]
fn openai_nested_function_name_without_wrapper() {
    // Bare OpenAI object (no `tool_call:` prefix, no array wrapper).
    // Currently we don't attempt to recover unprefixed bare JSON — users
    // would want an extractor that checks every `{` prefix, which is far
    // too aggressive for prose. This test locks in the "no false positive"
    // expectation.
    let text =
        r#"{"id":"c1","type":"function","function":{"name":"bash","arguments":{"command":"ls"}}}"#;
    let (calls, _) = extract_text_tool_calls(text);
    assert_eq!(
        calls.len(),
        0,
        "bare OpenAI JSON without a marker must NOT be auto-extracted"
    );
}

#[test]
fn extract_singular_tool_call_envelope() {
    let text = r#"{"tool_call":{"name":"bash","arguments":{"command":"ls -la"}}}"#;
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "bash");
    assert_eq!(calls[0].1["command"], "ls -la");
    assert!(cleaned.trim().is_empty());
}

#[test]
fn extract_singular_envelope_with_malformed_json_missing_colons() {
    // Seen in logs 2026-04-17 03:07 — Qwen dropped the colons after keys
    // in its hallucinated envelope. Strict serde_json refuses this; the
    // regex fallback must still recover name + primitive-valued args so
    // the tool actually executes.
    let text = r#"{"tool_call" {"name" "bash" "arguments" {"command" "git status"}}}"#;
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert_eq!(
        calls.len(),
        1,
        "malformed singular envelope must be recovered"
    );
    assert_eq!(calls[0].0, "bash");
    assert_eq!(calls[0].1["command"], "git status");
    assert!(cleaned.trim().is_empty());
}

#[test]
fn singular_envelope_rejects_plural_match() {
    // Sanity check — `"tool_calls"` (plural) must not be caught by the
    // singular branch. The plural has its own handling.
    let text = r#"{"tool_calls":[{"id":"c1","type":"function","function":{"name":"bash","arguments":{"command":"ls"}}}]}"#;
    let (calls, _) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "bash");
    assert_eq!(calls[0].1["command"], "ls");
}

#[test]
fn extract_claude_style_bash_invocation() {
    // Regression for 2026-04-17 14:27 — unsloth Qwen emitted a
    // `<bash><command>...</command></bash>` block and our parser missed
    // it entirely because the outer tag was the tool NAME, not one of
    // our known `<tool_call>` / `<function=` markers.
    let text = "Let me search for the OpenCode repo.\n\n\
        <bash>\n\
        <command>\n\
        curl -s \"https://api.github.com/search/repositories?q=opencode+oauth\" | python3 -c \"import json,sys; print(json.load(sys.stdin))\"\n\
        </command>\n\
        </bash>";
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "bash");
    assert!(
        calls[0].1["command"]
            .as_str()
            .unwrap_or("")
            .starts_with("curl"),
        "command arg must round-trip"
    );
    assert!(cleaned.contains("Let me search"));
    assert!(!cleaned.contains("<bash>"));
    assert!(!cleaned.contains("</bash>"));
}

#[test]
fn claude_style_multiple_params() {
    let text = "<edit_file>\n\
        <path>src/main.rs</path>\n\
        <old_string>foo</old_string>\n\
        <new_string>bar</new_string>\n\
        </edit_file>";
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "edit_file");
    assert_eq!(calls[0].1["path"], "src/main.rs");
    assert_eq!(calls[0].1["old_string"], "foo");
    assert_eq!(calls[0].1["new_string"], "bar");
    assert!(cleaned.trim().is_empty());
}

#[test]
fn claude_style_ignores_unknown_tag_names() {
    // `<html>` and `<script>` aren't in KNOWN_TOOL_NAMES — prose mentions
    // of HTML in a chat response must NOT get extracted as tool calls.
    let text = "The page has a <html><body>Hello</body></html> structure.";
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 0);
    assert_eq!(cleaned, text);
}

#[test]
fn tool_calls_inside_prose_is_ignored() {
    // Prose like `the "tool_calls" field carries tool calls` must not
    // match — the wrapping `{` is far away and belongs to something else.
    let text = r#"The field called "tool_calls" is an array. Another sentence."#;
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 0);
    assert_eq!(cleaned, text);
}

// --- Bare top-level array of OpenAI envelopes ---
// Seen 2026-05-16 with qwen-3.6-max-preview-thinking: the model double-emits,
// putting the full `[{"id":"call_...","type":"function",...}]` array into
// `delta.content` while the real call still flows via `delta.tool_calls`. The
// text copy used to bleed to the TUI as raw JSON. These tests pin the cleanup.

#[test]
fn balanced_json_accepts_arrays() {
    // The balanced extractor must now handle `[...]` too, since the new
    // bare-array pass uses it. Nested arrays and objects-inside-arrays
    // remain correctly bracketed.
    assert_eq!(extract_balanced_json(r#"[1,2,3]"#), Some(7));
    assert_eq!(
        extract_balanced_json(r#"[{"a":1},{"b":2}] trailing"#),
        Some(17)
    );
    assert_eq!(extract_balanced_json(r#"[1,2"#), None);
}

#[test]
fn extract_bare_array_single_call_compact() {
    let text = r#"Sure! [{"id":"call_1","type":"function","function":{"name":"bash","arguments":{"command":"ls"}}}] done."#;
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 1, "got {:?}", calls);
    assert_eq!(calls[0].0, "bash");
    assert_eq!(calls[0].1["command"], "ls");
    assert!(!cleaned.contains("call_1"));
    assert!(cleaned.contains("Sure!"));
    assert!(cleaned.contains("done"));
}

#[test]
fn extract_bare_array_pretty_printed_matches_log_shape() {
    // Verbatim text shape from ~/.opencrabs/logs/opencrabs.2026-05-16
    // (qwen-3.6-max-preview-thinking, ~20:16 UTC+1).
    let text = "Good idea. I'll add automatic sitemap discovery.\n\n[\n  {\n    \"id\": \"call_1\",\n    \"type\": \"function\",\n    \"function\": {\n      \"name\": \"edit_file\",\n      \"arguments\": {\n        \"path\": \"/x/scraper.rs\",\n        \"operation\": \"replace\",\n        \"old_text\": \"foo\",\n        \"new_text\": \"bar\"\n      }\n    }\n  }\n]";
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 1, "got {:?}", calls);
    assert_eq!(calls[0].0, "edit_file");
    assert_eq!(calls[0].1["operation"], "replace");
    assert!(cleaned.contains("Good idea"));
    assert!(!cleaned.contains("call_1"));
    assert!(!cleaned.contains("edit_file"));
}

#[test]
fn extract_bare_array_multiple_calls() {
    let text = r#"[{"id":"call_a","type":"function","function":{"name":"bash","arguments":{"command":"git status"}}},{"id":"call_b","type":"function","function":{"name":"read_file","arguments":{"path":"/x"}}}]"#;
    let (calls, _cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 2, "got {:?}", calls);
    assert_eq!(calls[0].0, "bash");
    assert_eq!(calls[1].0, "read_file");
    assert_eq!(calls[1].1["path"], "/x");
}

#[test]
fn extract_bare_array_with_stringified_arguments() {
    // OpenAI envelope often serializes `arguments` as a string, not an object.
    let text = r#"[{"id":"call_1","type":"function","function":{"name":"glob","arguments":"{\"pattern\":\"**/*.rs\"}"}}]"#;
    let (calls, _) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 1, "got {:?}", calls);
    assert_eq!(calls[0].0, "glob");
    assert_eq!(calls[0].1["pattern"], "**/*.rs");
}

#[test]
fn bare_array_anchor_requires_call_prefix() {
    // `"id":1` (numeric) or `"id":"x_1"` (non-call) must NOT trigger the
    // bare-array pass — that would over-match arbitrary JSON content. The
    // `"call_"` prefix is the OpenAI tool-call ID convention and is the
    // anchor we use for cheap pre-rejection.
    let text = r#"Here is JSON: [{"id":"banana_1","type":"function","function":{"name":"foo","arguments":{}}}] end."#;
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 0);
    assert_eq!(cleaned, text);
}

#[test]
fn bare_array_with_prose_id_call_mention_is_ignored() {
    // Prose mentioning `"id":"call_xyz"` without a wrapping `[` shortly
    // before must not match.
    let text = r#"The field "id":"call_xyz" is what OpenAI returns."#;
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 0);
    assert_eq!(cleaned, text);
}

#[test]
fn classify_bare_tool_array_states() {
    use BareToolArrayMatch::*;
    // Empty / whitespace-only inputs are valid prefixes (no info yet).
    assert_eq!(classify_bare_tool_array(""), Prefix);
    assert_eq!(classify_bare_tool_array("   "), Prefix);
    assert_eq!(classify_bare_tool_array("\n\n"), Prefix);

    // Each step along the recognition path is still a Prefix.
    assert_eq!(classify_bare_tool_array("["), Prefix);
    assert_eq!(classify_bare_tool_array("[\n"), Prefix);
    assert_eq!(classify_bare_tool_array("[ {"), Prefix);
    assert_eq!(classify_bare_tool_array("[\n  {\n    \"id\""), Prefix);
    assert_eq!(classify_bare_tool_array("[{\"id\":"), Prefix);
    assert_eq!(classify_bare_tool_array("[{\"id\":\"cal"), Prefix);

    // Complete recognition.
    assert_eq!(classify_bare_tool_array("[{\"id\":\"call_"), Full);
    assert_eq!(classify_bare_tool_array("[ {\"id\": \"call_1\"}]"), Full);
    assert_eq!(
        classify_bare_tool_array("[\n  {\n    \"id\": \"call_abc\"\n  }\n]"),
        Full
    );

    // Definite divergences.
    assert_eq!(classify_bare_tool_array("Hello"), None);
    assert_eq!(classify_bare_tool_array("{not array"), None);
    assert_eq!(classify_bare_tool_array("[1,2,3]"), None);
    assert_eq!(classify_bare_tool_array("[{\"name\":\"x\"}]"), None);
    assert_eq!(classify_bare_tool_array("[{\"id\":42}]"), None);
    assert_eq!(classify_bare_tool_array("[{\"id\":\"banana_\""), None);
}

// ── Dict-by-call-id shape (2026-05-24 qwen-3.7-max-preview regression) ──

#[test]
fn extract_dict_by_call_id_single() {
    // The exact shape seen in @adolfousier's Telegram screenshots:
    // top-level object keyed by call_<hex>, value is {name, arguments}.
    let text = r#"{"call_5f8d9c7b4a3e2f1c8d6e5b9a": {"name": "read_file", "arguments": {"path": "/tmp/x"}}}"#;
    let (calls, remaining) = extract_text_tool_calls(text);
    assert_eq!(
        calls.len(),
        1,
        "should extract one tool call from dict-by-id"
    );
    assert_eq!(calls[0].0, "read_file");
    assert_eq!(calls[0].1["path"], "/tmp/x");
    assert!(
        remaining.trim().is_empty(),
        "the matched object should be stripped from the remaining text, got: {remaining:?}",
    );
}

#[test]
fn extract_dict_by_call_id_multiple_keys() {
    // Some emissions cram several calls into one object.
    let text = r#"{"call_aaa": {"name":"bash","arguments":{"command":"ls"}},"call_bbb": {"name":"read_file","arguments":{"path":"/x"}}}"#;
    let (calls, _) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].0, "bash");
    assert_eq!(calls[1].0, "read_file");
}

#[test]
fn extract_dict_by_call_id_inside_markdown_fence() {
    // Telegram screenshots show the dict wrapped in ```json fences.
    // The fence isn't part of the JSON; we still want to find and strip
    // the inner object.
    let text = "Pushed fix.\n\n```json\n{\"call_7b8f9e6d4c5a3b2e1d7c6a9f\": {\"name\": \"bash\", \"arguments\": {\"command\": \"git push\"}}}\n```\n\nMore prose after.";
    let (calls, _remaining) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "bash");
    assert_eq!(calls[0].1["command"], "git push");
}

#[test]
fn dict_by_call_id_prose_mention_ignored() {
    // Prose like `the field "call_id" is set` must not trigger extraction:
    // the `"call_` substring exists but isn't the first key of an object.
    let text = r#"The "call_id" field is set in your config."#;
    let (calls, remaining) = extract_text_tool_calls(text);
    assert!(calls.is_empty(), "prose `call_id` must not be extracted");
    assert_eq!(remaining, text, "prose must pass through unchanged");
}

#[test]
fn dict_by_call_id_with_function_envelope_form() {
    // Some models nest under `function: {name, arguments}` even within
    // the dict-by-id shape — parse_tool_call_value already handles that.
    let text = r#"{"call_xyz": {"type":"function","function":{"name":"bash","arguments":"{\"command\":\"pwd\"}"}}}"#;
    let (calls, _) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "bash");
    assert_eq!(calls[0].1["command"], "pwd");
}
