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
fn openai_nested_function_name_recovered_via_bare_name_args_pass() {
    // Bare OpenAI envelope with no `tool_call:` prefix, no array wrapper,
    // and no `call_` id. The structured-tool_calls path can't see it,
    // and none of the marker-based passes claim it either. Before
    // 2026-06-02 this test asserted "no extraction" as a "we won't
    // over-match prose" guard — but real production logs showed the
    // model emitting exactly this shape as a JSON-stringified copy of
    // a tool call that did dispatch via the structured path. We need
    // to strip it; the bare_tool_call_extractor pass (Pass 1.9) now
    // recovers the inner `function: {name, arguments}` object as a
    // tool call. The KNOWN_TOOL_NAMES gate protects against this
    // turning into a false-positive on arbitrary prose JSON.
    let text =
        r#"{"id":"c1","type":"function","function":{"name":"bash","arguments":{"command":"ls"}}}"#;
    let (calls, _) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "bash");
    assert_eq!(calls[0].1["command"], "ls");
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

// ─── Pass 1.7 — bare-args orphans (qwen-3.7-max-thinking on dialagram) ──

#[test]
fn extract_bare_command_args_single() {
    let text = r#"{"command": "cd ~/srv/rs/opencrabs && cat src/rtk/tracker.rs | head -150"}"#;
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 1, "must synthesize a bash call");
    assert_eq!(calls[0].0, "bash");
    assert_eq!(
        calls[0].1["command"],
        "cd ~/srv/rs/opencrabs && cat src/rtk/tracker.rs | head -150"
    );
    assert!(
        cleaned.trim().is_empty(),
        "bare-args JSON must be stripped from visible text, got: {cleaned:?}"
    );
}

#[test]
fn extract_bare_command_args_multiple_blobs_user_screenshot() {
    // Verbatim reproduction of the 2026-05-27 user screenshot: five bare
    // `{"command": "..."}` blobs the model emitted in delta.content after
    // some structured tool calls succeeded. Pre-fix: every blob rendered
    // as visible JSON in the TUI and the calls never dispatched.
    let text = r#"{"command": "cd ~/srv/rs/opencrabs && cat src/rtk/tracker.rs | head -150"}
{"command": "cd ~/srv/rs/opencrabs && cat src/rtk/mod.rs | head -200"}
{"command": "cd ~/srv/rs/opencrabs && grep -r \"rtk::rewrite\\|rtk_rewrite\\|Rtk Result\" src/brain/tools/ --include=\"*.rs\" | head -20"}
{"command": "cd ~/srv/rs/opencrabs && git diff v0.3.24..v0.3.25 -- src/brain/tools/bash.rs | head -100"}
{"command": "cd ~/srv/rs/opencrabs && cat src/brain/tools/bash.rs | grep -A 30 -B 5 'rtk\\|spawn_blocking\\|block_on' | head -80"}"#;

    let (calls, cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 5, "all five blobs must become tool calls");
    for call in &calls {
        assert_eq!(call.0, "bash");
        assert!(
            call.1["command"]
                .as_str()
                .unwrap()
                .starts_with("cd ~/srv/rs/opencrabs"),
            "command must round-trip exactly: {call:?}"
        );
    }
    assert!(
        cleaned.trim().is_empty(),
        "all bare-args blobs must be stripped, leftover: {cleaned:?}"
    );
}

#[test]
fn extract_bare_command_args_with_optional_keys() {
    // Bash schema has `working_dir` and `timeout_secs` alongside `command`.
    // A bare-args object using those is still bash.
    let text = r#"{"command": "cargo build", "timeout_secs": 600}"#;
    let (calls, _) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "bash");
    assert_eq!(calls[0].1["command"], "cargo build");
    assert_eq!(calls[0].1["timeout_secs"], 600);
}

#[test]
fn bare_command_args_ignored_when_extra_unknown_keys() {
    // A prose-emitted object with `command` AND unknown extras (`note`,
    // `id`, etc.) is NOT a tool call — must not synthesize bash, must
    // leave content alone.
    let text = r#"{"command": "ls", "note": "example", "id": "x"}"#;
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert!(
        calls.is_empty(),
        "object with non-bash keys must not synthesize"
    );
    assert!(
        cleaned.contains("ls"),
        "non-tool prose must pass through, got: {cleaned:?}"
    );
}

#[test]
fn bare_command_args_ignored_when_command_not_string() {
    let text = r#"{"command": 42}"#;
    let (calls, _) = extract_text_tool_calls(text);
    assert!(calls.is_empty(), "non-string command must not synthesize");
}

#[test]
fn bare_command_args_alongside_prose_strips_only_json() {
    let text = "Running the build now: {\"command\": \"cargo build\"} please wait.";
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "bash");
    assert_eq!(calls[0].1["command"], "cargo build");
    assert!(
        cleaned.contains("Running the build") && cleaned.contains("please wait"),
        "surrounding prose must remain, got: {cleaned:?}"
    );
}

// ─── Pass 1.8 — invoke-style Anthropic XML (qwen:tool_call wrapper) ──

#[test]
fn invoke_style_single_clean_with_qwen_wrapper() {
    let text = r#"<qwen:tool_call>
<invoke name="read_file">
<parameter name="path">/tmp/x.dart</parameter>
<parameter name="start_line">10</parameter>
<parameter name="line_count">50</parameter>
</invoke>
</qwen:tool_call>"#;
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "read_file");
    assert_eq!(calls[0].1["path"], "/tmp/x.dart");
    assert_eq!(calls[0].1["start_line"], 10);
    assert_eq!(calls[0].1["line_count"], 50);
    assert!(
        cleaned.trim().is_empty(),
        "wrapper + invoke must be stripped, leftover: {cleaned:?}"
    );
}

#[test]
fn invoke_style_user_screenshot_2026_05_27_malformed() {
    // Verbatim reproduction of the 2026-05-27 Telegram screenshot.
    // Two malformations preserved:
    //   (a) first invoke missing leading `<`  →  `invoke name="read_file">`
    //   (b) second block wrote `<parameter name="read_file">` instead of
    //       `<invoke name="read_file">`
    let text = r#"<qwen:tool_call>
invoke name="read_file">
<parameter name="path">~/srv/dart/myapp/lib/presentation/buyer_intent_screen/widgets/chat_widget.dart</parameter>
<parameter name="start_line">1888</parameter>
<parameter name="line_count">50</parameter>
</invoke>
<parameter name="read_file">
<parameter name="path">~/srv/dart/myapp/lib/presentation/propositions_screen/propositions_screen.dart</parameter>
<parameter name="start_line">360</parameter>
<parameter name="line_count">30</parameter>
</invoke>
</qwen:tool_call>"#;
    let (calls, cleaned) = extract_text_tool_calls(text);

    // Both invokes must be recovered despite the malformations.
    assert_eq!(
        calls.len(),
        2,
        "both invokes (missing-< and parameter-not-invoke) must recover, got {} calls: {:?}",
        calls.len(),
        calls
    );

    // First call: `invoke name="read_file">` missing the leading `<`.
    assert_eq!(calls[0].0, "read_file");
    assert!(
        calls[0].1["path"]
            .as_str()
            .unwrap()
            .ends_with("chat_widget.dart"),
    );
    assert_eq!(calls[0].1["start_line"], 1888);
    assert_eq!(calls[0].1["line_count"], 50);

    // Second call: `<parameter name="read_file">` malformation.
    assert_eq!(calls[1].0, "read_file");
    assert!(
        calls[1].1["path"]
            .as_str()
            .unwrap()
            .ends_with("propositions_screen.dart"),
    );
    assert_eq!(calls[1].1["start_line"], 360);
    assert_eq!(calls[1].1["line_count"], 30);

    // Whole wrapper must be stripped from visible content.
    assert!(
        !cleaned.contains("qwen:tool_call"),
        "qwen wrapper must be gone from rendered content, got: {cleaned:?}"
    );
    assert!(
        !cleaned.contains("<invoke") && !cleaned.contains("invoke name="),
        "invoke blocks must be gone, got: {cleaned:?}"
    );
}

#[test]
fn invoke_style_anthropic_function_calls_wrapper() {
    let text = r#"<function_calls>
<invoke name="bash">
<parameter name="command">ls -la</parameter>
</invoke>
</function_calls>"#;
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "bash");
    assert_eq!(calls[0].1["command"], "ls -la");
    assert!(
        !cleaned.contains("function_calls"),
        "function_calls wrapper must be stripped, got: {cleaned:?}"
    );
}

#[test]
fn invoke_style_standalone_no_wrapper() {
    let text = r#"<invoke name="bash">
<parameter name="command">pwd</parameter>
</invoke>"#;
    let (calls, _cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "bash");
    assert_eq!(calls[0].1["command"], "pwd");
}

#[test]
fn invoke_style_unknown_tool_name_ignored() {
    // Prose mention with `invoke name="not_a_tool"` must NOT match.
    let text = r#"This is just prose with <invoke name="not_a_real_tool"> mentioned."#;
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert!(
        calls.is_empty(),
        "unknown tool name must not dispatch a call"
    );
    // Prose passes through unchanged (Pass 1.8 short-circuits, but the
    // overall extractor returns the text intact when no other pass strips).
    assert!(cleaned.contains("just prose"));
}

#[test]
fn invoke_style_value_type_coercion() {
    let text = r#"<invoke name="read_file">
<parameter name="path">/tmp/x</parameter>
<parameter name="start_line">42</parameter>
<parameter name="hashline">true</parameter>
</invoke>"#;
    let (calls, _) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].1["path"], "/tmp/x");
    assert_eq!(
        calls[0].1["start_line"], 42,
        "numeric string must coerce to integer"
    );
    assert_eq!(
        calls[0].1["hashline"], true,
        "boolean string must coerce to bool"
    );
}

#[test]
fn invoke_style_single_quoted_name_attribute() {
    let text = r#"<invoke name='bash'>
<parameter name='command'>echo hi</parameter>
</invoke>"#;
    let (calls, _) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "bash");
    assert_eq!(calls[0].1["command"], "echo hi");
}

// === Pass 1.7: bare `{"name": "<tool>", "arguments": {...}}` ===
//
// 2026-06-02 qwen-3.7-max-thinking via dialagram leaked tool calls
// as JSON-stringified objects inside delta.content while ALSO
// emitting the proper structured tool_calls delta. The structured
// path executed the call; the text copy made it to Telegram as raw
// JSON that looked like the user had pasted the tool args into the
// chat. These tests pin the bare-object detection and dedupe.

#[test]
fn bare_name_args_with_arguments_object_is_stripped_and_extracted() {
    // Exact shape from the 2026-06-02 leak (truncated arguments for
    // brevity, keys preserved). Bare object dropped in prose: no
    // <tool_call> wrapper, no `tool_call:` marker, no envelope.
    let text = r#"I see the issue now. Let me fix it.

{"name": "edit_file", "arguments": {"path": "/tmp/x.dart", "operation": "replace", "old_text": "foo", "new_text": "bar"}}

Then we'll verify."#;
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 1, "bare object must extract as one call");
    assert_eq!(calls[0].0, "edit_file");
    assert_eq!(calls[0].1["path"], "/tmp/x.dart");
    assert_eq!(calls[0].1["operation"], "replace");
    assert!(
        !cleaned.contains("\"name\": \"edit_file\""),
        "the bare JSON must be stripped from the visible text — \
         leak shape that hit Telegram on 2026-06-02 must not survive"
    );
    assert!(cleaned.contains("I see the issue now."));
    assert!(cleaned.contains("Then we'll verify."));
}

#[test]
fn bare_name_args_deduplicated_against_already_parsed_call() {
    // The model emits the SAME call twice in one assistant turn: once
    // bare in the prose and once via a structured tool_calls delta
    // (which is parsed upstream and shows up as `<tool_call>{json}</tool_call>`
    // when re-serialised into text for this extractor's input). The
    // bare copy must strip but NOT add a second call to the list.
    let text = r#"Let me fix it. <tool_call>{"name":"edit_file","arguments":{"path":"/tmp/x","operation":"append","new_text":"y"}}</tool_call>

{"name": "edit_file", "arguments": {"path": "/tmp/x", "operation": "append", "new_text": "y"}}"#;
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert_eq!(
        calls.len(),
        1,
        "the bare-copy duplicate must NOT add a second tool call — \
         the structured-path call already executed the same args"
    );
    assert_eq!(calls[0].0, "edit_file");
    assert!(
        !cleaned.contains("\"name\": \"edit_file\""),
        "both the wrapped AND the bare JSON must be stripped from \
         the visible text — only the prose lead-in should remain"
    );
}

#[test]
fn bare_name_args_with_string_arguments_json_string_is_parsed() {
    // Some providers double-encode: the model emits `arguments` as
    // a JSON-stringified string instead of a nested object. The
    // extractor must json::from_str the string before populating
    // the call's args (matches the structured-path normalisation).
    let text = r#"{"name": "bash", "arguments": "{\"command\":\"pwd\"}"}"#;
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "bash");
    assert_eq!(calls[0].1["command"], "pwd");
    assert!(cleaned.trim().is_empty());
}

#[test]
fn bare_name_args_requires_both_name_and_arguments_keys() {
    // Object with `name` but no `arguments` (e.g. legitimate prose
    // JSON like `{"name": "Adolfo"}`) must NOT trigger the extractor.
    // The signal AND the per-object key check both have to fail.
    let text = r#"Here's an example: {"name": "Adolfo", "role": "user"}. That's it."#;
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert!(
        calls.is_empty(),
        "a `name`-only object without `arguments` is not a tool call \
         envelope and must not be stripped or executed"
    );
    assert!(
        cleaned.contains("\"name\": \"Adolfo\""),
        "the legit prose JSON must survive — only tool-call-shaped \
         objects get stripped"
    );
}

#[test]
fn bare_name_args_inside_nested_object_is_ignored() {
    // `"name"` appearing as a nested key (not the FIRST key of its
    // parent object) must not be mistaken for a tool-call envelope.
    // Anchoring on the preceding `{` plus whitespace protects against
    // this — `{"foo": ..., "name": ..., "arguments": ...}` has `"name"`
    // preceded by `,`, not by `{`, so the pass skips it.
    let text =
        r#"The config is {"version": 2, "name": "edit_file", "arguments": {"path": "/tmp"}}."#;
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert!(
        calls.is_empty(),
        "`name` mid-object (not the first key) is not a tool-call \
         envelope; nothing must dispatch"
    );
    assert!(cleaned.contains("The config is"));
}

#[test]
fn bare_name_args_multiple_calls_in_one_message_extract_in_order() {
    // Two bare tool-call objects in one assistant message — both
    // must extract, both must strip, both must dispatch.
    let text = r#"First read, then edit.

{"name": "read_file", "arguments": {"path": "/tmp/a"}}

{"name": "edit_file", "arguments": {"path": "/tmp/a", "operation": "append", "new_text": "x"}}

Done."#;
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].0, "read_file");
    assert_eq!(calls[1].0, "edit_file");
    assert!(!cleaned.contains("\"name\": \"read_file\""));
    assert!(!cleaned.contains("\"name\": \"edit_file\""));
    assert!(cleaned.contains("First read, then edit."));
    assert!(cleaned.contains("Done."));
}

// ── `<tool_call_list>` wrapper (mimo-v2.5-pro leaks it into content) ──────

#[test]
fn tool_call_list_single_object() {
    let text = "<tool_call_list>\n{\"tool_call_id\": \"rsi_read\", \"tool_name\": \"self_improve\", \"tool_input\": {\"action\": \"read\", \"target_file\": \"USER.md\"}}\n</tool_call_list>";
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "self_improve");
    assert_eq!(calls[0].1["action"], "read");
    assert_eq!(calls[0].1["target_file"], "USER.md");
    assert!(!cleaned.contains("tool_call_list"));
}

#[test]
fn tool_call_list_keeps_surrounding_prose() {
    let text = "Running checks now.\n\n<tool_call_list>{\"tool_name\": \"bash\", \"tool_input\": {\"command\": \"cargo test\"}}</tool_call_list>";
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "bash");
    assert_eq!(calls[0].1["command"], "cargo test");
    assert!(cleaned.contains("Running checks now."));
    assert!(!cleaned.contains("tool_call_list"));
}

#[test]
fn tool_call_list_multiple_objects() {
    // Newline-separated objects inside one block.
    let text = "<tool_call_list>\n{\"tool_name\": \"read_file\", \"tool_input\": {\"path\": \"a.rs\"}}\n{\"tool_name\": \"edit_file\", \"tool_input\": {\"path\": \"b.rs\"}}\n</tool_call_list>";
    let (calls, _cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].0, "read_file");
    assert_eq!(calls[1].0, "edit_file");
}

#[test]
fn tool_call_list_json_array() {
    let text = "<tool_call_list>[{\"tool_name\": \"read_file\", \"tool_input\": {\"path\": \"a.rs\"}}, {\"tool_name\": \"bash\", \"tool_input\": {\"command\": \"ls\"}}]</tool_call_list>";
    let (calls, _cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].0, "read_file");
    assert_eq!(calls[1].0, "bash");
}

#[test]
fn tool_call_list_open_ended_no_closing_tag() {
    let text =
        "<tool_call_list>{\"tool_name\": \"bash\", \"tool_input\": {\"command\": \"echo hi\"}}";
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "bash");
    assert!(!cleaned.contains("tool_call_list"));
}

#[test]
fn tool_input_alias_maps_to_arguments() {
    // The `tool_input` key (mimo) must resolve to the call arguments.
    let text =
        "<tool_call>{\"tool_name\": \"bash\", \"tool_input\": {\"command\": \"pwd\"}}</tool_call>";
    let (calls, _cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "bash");
    assert_eq!(calls[0].1["command"], "pwd");
}

// ── Element-style <function><name> leak (#398, deepseek-v4-flash) ──

#[test]
fn element_style_function_block_extracts_and_strips() {
    let text = "Saving the payslip pipeline to MEMORY.md now.\n\n<function>\n<name>read_file</name>\n<path>/root/.opencrabs/MEMORY.md</path>\n</function>";
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 1, "calls: {calls:?}");
    assert_eq!(calls[0].0, "read_file");
    assert_eq!(
        calls[0].1.get("path").and_then(|v| v.as_str()),
        Some("/root/.opencrabs/MEMORY.md")
    );
    assert!(cleaned.contains("Saving the payslip pipeline"));
    assert!(!cleaned.contains("<function>"), "cleaned: {cleaned}");
    assert!(!cleaned.contains("<name>"));
}

#[test]
fn element_style_missing_closer_and_typed_args() {
    // Closing </function> dropped; numeric arg keeps its JSON type.
    let text =
        "<function>\n<name>read_file</name>\n<path>/tmp/x.md</path>\n<max_chars>500</max_chars>";
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].1.get("max_chars").and_then(|v| v.as_i64()),
        Some(500)
    );
    assert!(cleaned.trim().is_empty(), "cleaned: {cleaned}");
}

#[test]
fn element_style_prose_about_function_tag_untouched() {
    // Unknown tool name inside <name> means prose/docs, not a call.
    let text = "Docs: a <function> tag wraps <name>whatever</name> in that format.";
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert!(calls.is_empty());
    assert_eq!(cleaned, text);
}

#[test]
fn element_style_multiple_blocks_all_extract() {
    let text = "<function><name>read_file</name><path>/a</path></function>\nthen\n<function><name>bash</name><command>ls</command></function>";
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 2, "calls: {calls:?}");
    assert_eq!(calls[0].0, "read_file");
    assert_eq!(calls[1].0, "bash");
    assert_eq!(
        calls[1].1.get("command").and_then(|v| v.as_str()),
        Some("ls")
    );
    assert!(cleaned.contains("then"));
    assert!(!cleaned.contains("<function>"));
}

#[test]
fn element_style_attributed_parameter_form() {
    // Second observed deepseek-v4-flash shape: <parameter name="k"> with
    // extra attributes, quoted content, and a </parameter> closer.
    let text = "Ahh, got it.\n\n<function>\n<name>bash</name>\n<parameter name=\"command\" string=\"true\">grep -c \"DM content stays DM\" /root/.opencrabs/SOUL.md</parameter>\n</function>";
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 1, "calls: {calls:?}");
    assert_eq!(calls[0].0, "bash");
    assert_eq!(
        calls[0].1.get("command").and_then(|v| v.as_str()),
        Some("grep -c \"DM content stays DM\" /root/.opencrabs/SOUL.md")
    );
    assert!(cleaned.contains("Ahh, got it."));
    assert!(!cleaned.contains("<function>"), "cleaned: {cleaned}");
}

#[test]
fn element_style_attributed_load_brain_file() {
    let text = "<function>\n<name>load_brain_file</name>\n<parameter name=\"name\" string=\"true\">SOUL.md</parameter>\n</function>";
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 1, "calls: {calls:?}");
    assert_eq!(calls[0].0, "load_brain_file");
    assert_eq!(
        calls[0].1.get("name").and_then(|v| v.as_str()),
        Some("SOUL.md")
    );
    assert!(cleaned.trim().is_empty());
}

#[test]
fn element_style_dsml_corrupted_closers() {
    // Live deepseek-v4-flash sample (#398): DSML tokenizer corruption turns
    // the closing tags into </｜｜DSML｜｜parameter> etc., with trailing
    // </｜｜DSML｜｜invoke> and </｜｜DSML｜｜tool_calls> garbage. The value
    // itself contains '>' (shell redirection), which must not confuse the
    // scanner.
    let text = "<function>\n<name>bash</name>\n<parameter name=\"command\" string=\"true\">opencrabs --version 2>/dev/null || echo \"binary not found at default path\"</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter>\n</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke>\n</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}tool_calls>";
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 1, "calls: {calls:?}");
    assert_eq!(calls[0].0, "bash");
    assert_eq!(
        calls[0].1.get("command").and_then(|v| v.as_str()),
        Some("opencrabs --version 2>/dev/null || echo \"binary not found at default path\"")
    );
    assert!(
        cleaned.trim().is_empty(),
        "no DSML garbage may survive: {cleaned}"
    );
}

#[test]
fn element_style_single_bar_dsml_variant() {
    let text = "<function>\n<name>read_file</name>\n<path>/tmp/a.md</\u{ff5c}DSML\u{ff5c}path>\n</\u{ff5c}DSML\u{ff5c}function>";
    let (calls, cleaned) = extract_text_tool_calls(text);
    assert_eq!(calls.len(), 1, "calls: {calls:?}");
    assert_eq!(
        calls[0].1.get("path").and_then(|v| v.as_str()),
        Some("/tmp/a.md")
    );
    assert!(cleaned.trim().is_empty(), "cleaned: {cleaned}");
}
