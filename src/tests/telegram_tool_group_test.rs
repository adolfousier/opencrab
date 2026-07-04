//! Tests for `telegram::handler::render_tool_group`.
//!
//! Grouped tool calls must render as native Telegram HTML using
//! `<blockquote expandable>` (Bot API 7.3+), never `<details>` — Telegram's
//! regular HTML parse mode does not support `<details>`, so that tag leaks
//! into the chat as literal text.

use crate::channels::telegram::handler::{FlowLine, render_flow_html};

fn tline(label: &str, context: &str) -> FlowLine {
    FlowLine::Tool {
        label: label.to_string(),
        context: context.to_string(),
    }
}

#[test]
fn empty_group_renders_nothing() {
    assert_eq!(render_flow_html(&[]), "");
}

#[test]
fn single_tool_renders_plain_line_without_blockquote() {
    let out = render_flow_html(&[tline("✅ bash", "git status")]);
    assert_eq!(out, "<b>✅ bash</b> git status");
    assert!(!out.contains("<blockquote"));
}

#[test]
fn single_tool_without_context_omits_trailing_space() {
    let out = render_flow_html(&[tline("⚙️ web_search", "")]);
    assert_eq!(out, "<b>⚙️ web_search</b>");
}

#[test]
fn multiple_tools_render_expandable_blockquote() {
    let out = render_flow_html(&[
        tline("✅ bash", "cargo fmt"),
        tline("✅ read_file", "handler.rs"),
        tline("❌ grep", "pattern"),
    ]);
    assert!(out.starts_with("<blockquote expandable><b>3 tool calls</b>\n"));
    assert!(out.ends_with("</blockquote>"));
    assert!(out.contains("<b>✅ bash</b> cargo fmt"));
    assert!(out.contains("<b>✅ read_file</b> handler.rs"));
    assert!(out.contains("<b>❌ grep</b> pattern"));
}

#[test]
fn never_emits_details_tags() {
    let out = render_flow_html(&[tline("✅ a", "x"), tline("✅ b", "y")]);
    assert!(!out.contains("<details>"));
    assert!(!out.contains("<summary>"));
}

#[test]
fn escapes_html_in_labels_and_context() {
    let out = render_flow_html(&[
        tline("✅ bash", "grep '<details>' & \"stuff\""),
        tline("✅ edit_file", "a < b > c"),
    ]);
    assert!(out.contains("grep '&lt;details&gt;' &amp; \"stuff\""));
    assert!(out.contains("a &lt; b &gt; c"));
    // No raw angle brackets from content survive outside our own tags
    assert!(!out.contains("'<details>'"));
}

// ── Mixed processing-log flow (tool calls + intermediate text) — #300 ──

#[test]
fn tool_plus_text_folds_into_one_blockquote() {
    let out = render_flow_html(&[
        tline("✅ bash", "git status"),
        FlowLine::Text("Checked the tree, all clean.".to_string()),
        tline("✅ read_file", "handler.rs"),
    ]);
    assert!(out.starts_with("<blockquote expandable><b>2 tool calls</b>\n"));
    assert!(out.ends_with("</blockquote>"));
    assert!(out.contains("<b>✅ bash</b> git status"));
    assert!(out.contains("Checked the tree, all clean."));
    assert!(out.contains("<b>✅ read_file</b> handler.rs"));
    assert!(!out.contains("<details>"));
}

#[test]
fn text_only_flow_uses_processing_log_header() {
    let out = render_flow_html(&[FlowLine::Text("Switching provider…".to_string())]);
    assert!(out.starts_with("<blockquote expandable><b>Processing log</b>\n"));
    assert!(out.contains("Switching provider…"));
    assert!(!out.contains("tool calls"));
}

#[test]
fn intermediate_text_is_html_escaped_in_flow() {
    let out = render_flow_html(&[
        tline("✅ bash", "echo hi"),
        FlowLine::Text("result: <b>bold</b> & <script>alert(1)</script>".to_string()),
    ]);
    assert!(out.contains("&lt;b&gt;bold&lt;/b&gt; &amp; &lt;script&gt;"));
    // The injected tags must never survive as live HTML.
    assert!(!out.contains("<script>"));
}

#[test]
fn blank_text_entries_are_dropped() {
    let out = render_flow_html(&[tline("✅ bash", "x"), FlowLine::Text("   ".to_string())]);
    // A lone tool with only blank text collapses to the one-liner.
    assert_eq!(out, "<b>✅ bash</b> x");
}

#[test]
fn empty_flow_renders_nothing() {
    assert_eq!(render_flow_html(&[]), "");
}
