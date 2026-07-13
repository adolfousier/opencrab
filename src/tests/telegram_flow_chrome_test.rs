//! Tests for the ADR 0005 flow-message structure: the always-visible plan
//! chrome (title / checklist / goal), the merged footer (status → log summary →
//! ctx → clock), and the uncollapsed shell — the whole message is never wrapped
//! in one outer expandable; only the processing log collapses.

use crate::channels::telegram::flow::{
    FlowHeader, FlowLine, HeaderMarkup, render_flow_details_chrome,
    render_flow_details_chrome_pref, render_flow_html_chrome, render_flow_html_chrome_pref,
};
use crate::channels::telegram::flow_chrome::{FlowSections, clock_glyph};

fn sections(title: Option<&str>, checklist: Option<Vec<&str>>, goal: Option<&str>) -> FlowSections {
    FlowSections {
        plan_state: None,
        plan_kb: Default::default(),
        plan_title: title.map(str::to_string),
        checklist: checklist.map(|rows| rows.into_iter().map(str::to_string).collect()),
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

// ── clock glyph (Decision 13) ──

#[test]
fn clock_glyph_formats_minutes_and_hours() {
    assert_eq!(clock_glyph(0), "⏱ 0:00");
    assert_eq!(clock_glyph(9), "⏱ 0:09");
    assert_eq!(clock_glyph(83), "⏱ 1:23");
    assert_eq!(clock_glyph(3665), "⏱ 1:01:05");
}

// ── chrome blocks: always-visible title / full checklist / goal (Decision 3) ──
// plan_state and ctx moved to the merged footer (Decision 7 / Decision 12), so
// they must NOT appear in the chrome blocks.

#[test]
fn chrome_blocks_order_title_checklist_rows_goal_and_omit_state_and_ctx() {
    let mut s = sections(
        Some("Ship plan mode"),
        Some(vec!["☑ scope it", "☐ build it"]),
        Some("close B"),
    );
    s.plan_state = Some("✍️ Editing plan".to_string());
    s.ctx = Some("ctx 12.3k/200k".to_string());
    let blocks = s.chrome_blocks(HeaderMarkup::Html);
    assert_eq!(
        blocks,
        vec![
            "📋 <b>Ship plan mode</b>".to_string(),
            "☑ scope it".to_string(),
            "☐ build it".to_string(),
            "🎯 <i>close B</i>".to_string(),
        ]
    );
    let joined = blocks.join("\n");
    assert!(
        !joined.contains("Editing plan"),
        "plan_state stays in the footer"
    );
    assert!(!joined.contains("ctx"), "ctx stays in the footer");
}

#[test]
fn chrome_blocks_empty_when_title_checklist_goal_all_empty() {
    let mut s = sections(None, None, None);
    // plan_state / ctx set but no title/checklist/goal → no chrome blocks.
    s.plan_state = Some("✍️ Editing plan".to_string());
    s.ctx = Some("ctx 1k/200k".to_string());
    assert!(s.chrome_blocks(HeaderMarkup::Html).is_empty());
}

#[test]
fn chrome_blocks_escape_html_in_section_text() {
    let s = sections(Some("a <b> & c"), Some(vec!["☐ x < y"]), None);
    let blocks = s.chrome_blocks(HeaderMarkup::Html);
    let joined = blocks.join("\n");
    assert!(joined.contains("a &lt;b&gt; &amp; c"), "title escaped");
    assert!(joined.contains("☐ x &lt; y"), "checklist row escaped");
    assert!(!joined.contains("a <b> & c"));
}

#[test]
fn chrome_blocks_markdown_dialect_keeps_raw_text() {
    let s = sections(Some("title"), Some(vec!["☐ do X"]), None);
    let blocks = s.chrome_blocks(HeaderMarkup::Markdown);
    assert_eq!(
        blocks,
        vec!["📋 **title**".to_string(), "☐ do X".to_string()]
    );
}

// ── header-only renders (empty flow_entries): plain merged footer ──

#[test]
fn header_only_html_is_plain_footer_line() {
    // Pre-tool phase, non-plan: no log, so a plain footer line with the
    // Working-on status and the clock — no blockquote, no <sub> on classic.
    let out = render_flow_html_chrome(
        &[],
        &FlowHeader::Live(Some("10s")),
        Some("Working on: fix the tests"),
        &FlowSections::default(),
        10,
    );
    assert_eq!(out, "Working on: fix the tests • ⏱ 0:10");
    assert!(!out.contains("<blockquote"), "no outer expandable");
    assert!(
        !out.contains("<details"),
        "no log details before first entry"
    );
}

#[test]
fn header_only_html_leads_with_chrome_then_footer() {
    let out = render_flow_html_chrome(
        &[],
        &FlowHeader::Live(None),
        None,
        &sections(Some("Ship it"), Some(vec!["☐ wire it", "☐ test it"]), None),
        0,
    );
    // Chrome leads (always visible): title then one line per checklist task; a
    // blank line separates it from the plain footer clock.
    assert_eq!(out, "📋 <b>Ship it</b>\n☐ wire it\n☐ test it\n\n⏱ 0:00");
}

#[test]
fn header_only_settled_no_tool_turn_puts_ctx_before_clock() {
    // Settled no-tool turn: footer = outcome → ctx → clock, ctx BEFORE the clock.
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
        3,
    );
    assert_eq!(out, "✅ Finished • ctx 9.1k/200k • ⏱ 0:03");
}

#[test]
fn header_only_details_is_plain_sub_footer_line() {
    let out = render_flow_details_chrome(
        &[],
        &FlowHeader::Live(Some("5s")),
        Some("🧠 reading the diff"),
        &FlowSections::default(),
        5,
    );
    assert_eq!(out, "<sub>🧠 reading the diff • ⏱ 0:05</sub>");
    assert!(
        !out.contains("<details>"),
        "no log details before first entry"
    );
}

// ── populated flows: uncollapsed shell, log in its own block, footer last ──

#[test]
fn html_populated_flow_has_no_outer_expandable() {
    let lines = [
        tline("✅ bash", "git status"),
        tline("⚙️ read_file", "a.rs"),
    ];
    let out = render_flow_html_chrome(
        &lines,
        &FlowHeader::Live(Some("20s")),
        None,
        &sections(
            Some("Plan"),
            Some(vec!["☑ first", "☐ second", "☐ third", "☐ fourth"]),
            None,
        ),
        20,
    );
    // Chrome leads and is always visible (title then full ☐/☑ list), not inside
    // any expandable.
    assert!(out.starts_with("📋 <b>Plan</b>\n☑ first\n☐ second\n☐ third\n☐ fourth\n\n"));
    assert!(
        !out.starts_with("<blockquote"),
        "the whole message must not be one outer expandable"
    );
    // The processing log lives in its OWN expandable, chrome outside it.
    assert!(out.contains("<blockquote expandable><b>✅ bash</b> <code>git status</code>"));
    assert!(
        out.contains("</blockquote>\n"),
        "footer is a plain line under the log"
    );
    // In-flight footer: cog on the log summary, clock last.
    assert!(out.contains("⚙️"), "in-flight log summary carries the cog");
    assert!(out.contains("2 tool calls"));
    assert!(out.ends_with("⏱ 0:20"), "clock is the last footer segment");
}

#[test]
fn details_populated_flow_keeps_chrome_outside_the_details() {
    let lines = [tline("✅ bash", "ls"), tline("✅ grep", "todo")];
    let out = render_flow_details_chrome(
        &lines,
        &FlowHeader::Live(Some("8s")),
        None,
        &sections(None, None, Some("finish the audit")),
        8,
    );
    // Chrome is an always-visible <p> block BEFORE the collapsed log, with a
    // kept spacer, not inside the summary.
    assert!(
        out.starts_with("<p>🎯 <i>finish the audit</i></p><p>&nbsp;</p><details><summary><sub>")
    );
    assert!(out.ends_with("</details>"));
    assert!(out.contains("⏱ 0:08"));
}

#[test]
fn footer_shows_both_working_on_status_and_activity_summary() {
    // ADR 0005 footer merge: Working-on is segment 1, the live activity is the
    // segment-2 log summary — both visible (the old "activity beats fallback"
    // collapsed-preview rule is gone).
    let lines = [
        tline("✅ bash", "ls"),
        FlowLine::Text("Now checking the config.".to_string()),
        tline("⚙️ read_file", "config.toml"),
    ];
    let out = render_flow_html_chrome(
        &lines,
        &FlowHeader::Live(Some("30s")),
        Some("Working on: ship it"),
        &FlowSections::default(),
        30,
    );
    assert!(
        out.contains("Working on: ship it"),
        "status segment present"
    );
    assert!(
        out.contains("Now checking the config."),
        "activity summary present"
    );
}

#[test]
fn single_tool_gets_its_own_log_block_and_footer() {
    // The lone-tool-plain shortcut is gone under ADR 0005: even one entry sits
    // in its own expandable with the footer below.
    let out = render_flow_html_chrome(
        &[tline("✅ bash", "git status")],
        &FlowHeader::Live(None),
        None,
        &sections(Some("Plan"), None, None),
        0,
    );
    assert!(out.starts_with("📋 <b>Plan</b>\n\n"));
    assert!(
        out.contains(
            "<blockquote expandable><b>✅ bash</b> <code>git status</code></blockquote>\n"
        )
    );
    assert!(out.contains("1 tool calls"));
    assert!(out.ends_with("⏱ 0:00"));
}

#[test]
fn settled_footer_drops_the_cog() {
    // Settled footer: outcome carries ✅/❌, the log summary is a bare tool
    // count with NO cog (Decision 4 / 12).
    let lines = [tline("✅ bash", "ls"), tline("✅ grep", "todo")];
    let out = render_flow_html_chrome(
        &lines,
        &FlowHeader::Settled {
            icon: "✅",
            verb: "Finished",
            duration: "2m",
        },
        None,
        &FlowSections::default(),
        124,
    );
    // Footer is the plain final line under the log block.
    let footer = out
        .rsplit("</blockquote>\n")
        .next()
        .expect("footer present");
    assert!(footer.starts_with("✅ Finished • 2 tool calls • ⏱ 2:04"));
    assert!(
        !footer.contains("⚙️"),
        "settled footer never carries the cog"
    );
}

#[test]
fn checklist_rows_render_as_separate_rich_paragraphs() {
    let out = render_flow_details_chrome(
        &[],
        &FlowHeader::Live(Some("2s")),
        None,
        &sections(Some("Plan"), Some(vec!["☑ done one", "☐ next"]), None),
        2,
    );
    // Rich: title and each checklist row are their own <p> block before the
    // kept spacer and the <sub> footer (rich HTML ignores raw newlines).
    assert!(
        out.starts_with("<p>📋 <b>Plan</b></p><p>☑ done one</p><p>☐ next</p><p>&nbsp;</p><sub>")
    );
}

#[test]
fn full_checklist_kept_when_all_tasks_done() {
    // Decision 9: the full list stays through settle even when every task is
    // ticked (the old N/M count hid a fully-done checklist).
    let out = render_flow_html_chrome(
        &[],
        &FlowHeader::Live(None),
        None,
        &sections(Some("Done plan"), Some(vec!["☑ a", "☑ b"]), None),
        0,
    );
    assert!(out.starts_with("📋 <b>Done plan</b>\n☑ a\n☑ b\n\n"));
}

// ── provider-aware folded-narration cap (#532 / upstream #531) ──────
// CLI providers fold the whole model turn into the block, so folded narration
// is capped (300); API providers pass uncapped (usize::MAX) and keep full
// reasoning. cap_narration is private, so these exercise it through the render
// path: a long narration line is truncated at the CLI cap and kept whole at the
// API cap.

fn long_narration(n: usize) -> String {
    "x".repeat(n)
}

fn narration_then_tool() -> [FlowLine; 2] {
    [
        FlowLine::Text(long_narration(1000)),
        tline("⚙️ read_file", "config.toml"),
    ]
}

#[test]
fn cli_cap_truncates_body_api_keeps_it_full_html() {
    let lines = narration_then_tool();
    let cli = render_flow_html_chrome_pref(
        &lines,
        &FlowHeader::Live(Some("2s")),
        None,
        &FlowSections::default(),
        300,
        2,
    );
    let api = render_flow_html_chrome_pref(
        &lines,
        &FlowHeader::Live(Some("2s")),
        None,
        &FlowSections::default(),
        usize::MAX,
        2,
    );
    assert!(
        cli.contains('…'),
        "CLI cap truncates the folded body with an ellipsis: {cli}"
    );
    assert!(
        api.chars().count() > cli.chars().count(),
        "API keeps the full folded body, CLI truncates it (cli={} api={})",
        cli.chars().count(),
        api.chars().count()
    );
    assert!(
        api.contains(&long_narration(1000)),
        "the uncapped API render keeps the whole 1000-char body entry"
    );
}

#[test]
fn cli_cap_truncates_body_api_keeps_it_full_details() {
    let lines = narration_then_tool();
    let cli = render_flow_details_chrome_pref(
        &lines,
        &FlowHeader::Live(Some("2s")),
        None,
        &FlowSections::default(),
        300,
        2,
    );
    let api = render_flow_details_chrome_pref(
        &lines,
        &FlowHeader::Live(Some("2s")),
        None,
        &FlowSections::default(),
        usize::MAX,
        2,
    );
    assert!(
        cli.contains('…'),
        "CLI cap truncates in the details path too"
    );
    assert!(
        api.chars().count() > cli.chars().count(),
        "API keeps the full folded body in the details path (cli={} api={})",
        cli.chars().count(),
        api.chars().count()
    );
}

#[test]
fn short_narration_untouched_by_either_cap() {
    let lines = [
        FlowLine::Text("brief note".to_string()),
        tline("⚙️ read_file", "config.toml"),
    ];
    for cap in [300usize, usize::MAX] {
        let out = render_flow_html_chrome_pref(
            &lines,
            &FlowHeader::Live(Some("2s")),
            None,
            &FlowSections::default(),
            cap,
            2,
        );
        assert!(out.contains("brief note"));
        assert!(
            !out.contains('…'),
            "short narration must never be truncated (cap={cap})"
        );
    }
}
