//! Tests for `telegram::handler::render_tool_group`.
//!
//! Grouped tool calls must render as native Telegram HTML using
//! `<blockquote expandable>` (Bot API 7.3+), never `<details>` — Telegram's
//! regular HTML parse mode does not support `<details>`, so that tag leaks
//! into the chat as literal text.

use crate::channels::telegram::handler::render_tool_group;

fn tool(label: &str, context: &str) -> (String, String) {
    (label.to_string(), context.to_string())
}

#[test]
fn empty_group_renders_nothing() {
    assert_eq!(render_tool_group(&[]), "");
}

#[test]
fn single_tool_renders_plain_line_without_blockquote() {
    let out = render_tool_group(&[tool("✅ bash", "git status")]);
    assert_eq!(out, "<b>✅ bash</b> git status");
    assert!(!out.contains("<blockquote"));
}

#[test]
fn single_tool_without_context_omits_trailing_space() {
    let out = render_tool_group(&[tool("⚙️ web_search", "")]);
    assert_eq!(out, "<b>⚙️ web_search</b>");
}

#[test]
fn multiple_tools_render_expandable_blockquote() {
    let out = render_tool_group(&[
        tool("✅ bash", "cargo fmt"),
        tool("✅ read_file", "handler.rs"),
        tool("❌ grep", "pattern"),
    ]);
    assert!(out.starts_with("<blockquote expandable><b>3 tool calls</b>\n"));
    assert!(out.ends_with("</blockquote>"));
    assert!(out.contains("<b>✅ bash</b> cargo fmt"));
    assert!(out.contains("<b>✅ read_file</b> handler.rs"));
    assert!(out.contains("<b>❌ grep</b> pattern"));
}

#[test]
fn never_emits_details_tags() {
    let out = render_tool_group(&[tool("✅ a", "x"), tool("✅ b", "y")]);
    assert!(!out.contains("<details>"));
    assert!(!out.contains("<summary>"));
}

#[test]
fn escapes_html_in_labels_and_context() {
    let out = render_tool_group(&[
        tool("✅ bash", "grep '<details>' & \"stuff\""),
        tool("✅ edit_file", "a < b > c"),
    ]);
    assert!(out.contains("grep '&lt;details&gt;' &amp; \"stuff\""));
    assert!(out.contains("a &lt; b &gt; c"));
    // No raw angle brackets from content survive outside our own tags
    assert!(!out.contains("'<details>'"));
}
