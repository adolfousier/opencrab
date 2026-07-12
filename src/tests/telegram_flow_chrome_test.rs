//! Tests for the flow-message chrome: always-visible sections (plan title,
//! checklist progress, active goal, ctx footer) and the header-only render
//! paths that let the flow message open before any tool body exists.

use crate::channels::telegram::flow::{
    FlowHeader, FlowLine, HeaderMarkup, render_flow_details_chrome, render_flow_html_chrome,
};
use crate::channels::telegram::flow_chrome::FlowSections;

fn sections(title: Option<&str>, checklist: Option<&str>, goal: Option<&str>) -> FlowSections {
    FlowSections {
        plan_state: None,
        plan_kb: Default::default(),
        plan_title: title.map(str::to_string),
        checklist: checklist.map(str::to_string),
        goal: goal.map(str::to_string),
        ctx: None,
    }
}

fn tline(label: &str, context: &str) -> FlowLine {
    FlowLine::Tool {
        label: label.to_string(),
        context: context.to_string(),
        raw_context: String::new(),
    }
}

// ── chrome_line: one compact section line, shared by all renderers ──

#[test]
fn chrome_line_orders_sections_and_omits_empty() {
    let mut s = sections(Some("Ship plan mode"), Some("2/7 tasks"), Some("close B"));
    s.ctx = Some("ctx 12.3k/200k".to_string());
    let line = s.chrome_line(HeaderMarkup::Html).expect("all sections set");
    assert_eq!(
        line,
        "📋 <b>Ship plan mode</b> • <i>2/7 tasks</i> • 🎯 <i>close B</i> • <i>ctx 12.3k/200k</i>"
    );
}

#[test]
fn chrome_line_none_when_all_sections_empty() {
    assert!(
        sections(None, None, None)
            .chrome_line(HeaderMarkup::Html)
            .is_none()
    );
}

#[test]
fn chrome_line_escapes_html_in_section_text() {
    let s = sections(Some("a <b> & c"), None, None);
    let line = s.chrome_line(HeaderMarkup::Html).expect("title set");
    assert!(line.contains("a &lt;b&gt; &amp; c"));
    assert!(!line.contains("a <b> & c"));
}

#[test]
fn chrome_line_markdown_dialect_keeps_raw_text() {
    let s = sections(Some("title"), Some("1/3 tasks"), None);
    let line = s.chrome_line(HeaderMarkup::Markdown).expect("sections set");
    assert_eq!(line, "📋 **title** • _1/3 tasks_");
}

// ── header-only renders (empty flow_entries) ──

#[test]
fn header_only_html_uses_fallback_preview() {
    // Pre-tool phase: no entries yield an activity preview, so the
    // thinking / Working-on fallback rides the header.
    let out = render_flow_html_chrome(
        &[],
        &FlowHeader::Live(Some("10s")),
        Some("Working on: fix the tests"),
        &FlowSections::default(),
    );
    assert_eq!(
        out,
        "⚙️ <b>Working on: fix the tests</b> • <i>Processing log</i> • <i>10s</i>"
    );
}

#[test]
fn header_only_html_appends_chrome_line() {
    let out = render_flow_html_chrome(
        &[],
        &FlowHeader::Live(None),
        None,
        &sections(Some("Ship it"), Some("0/2 tasks"), None),
    );
    assert_eq!(
        out,
        "<b>Processing log</b>\n📋 <b>Ship it</b> • <i>0/2 tasks</i>"
    );
}

#[test]
fn header_only_settled_no_tool_turn_keeps_ctx() {
    // A settled no-tool turn: the flow message stays as chrome, ctx on it.
    let secs = FlowSections {
        ctx: Some("ctx 9.1k/200k".to_string()),
        ..Default::default()
    };
    let out = render_flow_html_chrome(
        &[],
        &FlowHeader::Settled {
            icon: "✅",
            verb: "Finished",
            duration: "3s",
        },
        None,
        &secs,
    );
    assert_eq!(out, "<b>✅ Finished (3s)</b>\n<i>ctx 9.1k/200k</i>");
}

#[test]
fn header_only_details_is_plain_summary_line() {
    let out = render_flow_details_chrome(
        &[],
        &FlowHeader::Live(Some("5s")),
        Some("🧠 reading the diff"),
        &FlowSections::default(),
    );
    assert_eq!(
        out,
        "<sub>⚙️ <b>🧠 reading the diff</b> • <i>Processing log</i> • <i>5s</i></sub>"
    );
    assert!(!out.contains("<details>"));
}

// ── chrome on populated flows ──

#[test]
fn html_block_carries_chrome_under_header() {
    let lines = [
        tline("✅ bash", "git status"),
        tline("⚙️ read_file", "a.rs"),
    ];
    let out = render_flow_html_chrome(
        &lines,
        &FlowHeader::Live(Some("20s")),
        None,
        &sections(Some("Plan"), Some("1/4 tasks"), None),
    );
    assert!(out.starts_with("<blockquote expandable>"));
    assert!(out.contains("📋 <b>Plan</b> • <i>1/4 tasks</i>\n\n"));
    assert!(out.contains("<b>✅ bash</b>"));
}

#[test]
fn details_summary_carries_chrome() {
    let lines = [tline("✅ bash", "ls"), tline("✅ grep", "todo")];
    let out = render_flow_details_chrome(
        &lines,
        &FlowHeader::Live(Some("8s")),
        None,
        &sections(None, None, Some("finish the audit")),
    );
    assert!(out.starts_with("<details><summary><sub>"));
    // Chrome must live in the summary so it stays visible when collapsed.
    let summary_end = out.find("</sub></summary>").expect("summary present");
    assert!(out[..summary_end].contains("🎯 <i>finish the audit</i>"));
}

#[test]
fn entry_preview_beats_fallback_preview() {
    // Once real activity exists, the latest-activity preview wins over the
    // pre-activity fallback.
    let lines = [
        tline("✅ bash", "ls"),
        FlowLine::Text("Now checking the config.".to_string()),
        tline("⚙️ read_file", "config.toml"),
    ];
    let out = render_flow_html_chrome(
        &lines,
        &FlowHeader::Live(Some("30s")),
        Some("Working on: stale preview"),
        &FlowSections::default(),
    );
    assert!(out.contains("Now checking the config."));
    assert!(!out.contains("stale preview"));
}

#[test]
fn lone_tool_line_stays_plain_even_with_chrome() {
    // #296: a lone live tool line stays a plain one-liner; chrome waits for
    // the block shape.
    let out = render_flow_html_chrome(
        &[tline("✅ bash", "git status")],
        &FlowHeader::Live(None),
        None,
        &sections(Some("Plan"), None, None),
    );
    assert_eq!(out, "<b>✅ bash</b> <code>git status</code>");
}
