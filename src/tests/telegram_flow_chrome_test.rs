//! Tests for the ADR 0005 flow-message structure: the always-visible plan
//! chrome (title / checklist / goal), the merged footer (status → log summary →
//! ctx → clock), and the uncollapsed shell — the whole message is never wrapped
//! in one outer expandable; only the processing log collapses.

use crate::channels::telegram::flow::{
    FlowHeader, FlowLine, render_flow_details_chrome, render_flow_details_chrome_pref,
    render_flow_html_chrome, render_flow_html_chrome_pref,
};
use crate::channels::telegram::flow_chrome::{
    FlowSections, GoalSection, ProseSection, clock_glyph, split_plan_prose,
};

fn sections(title: Option<&str>, checklist: Option<Vec<&str>>, goal: Option<&str>) -> FlowSections {
    FlowSections {
        plan_state: None,
        plan_kb: Default::default(),
        plan_title: title.map(str::to_string),
        prose: None,
        checklist: checklist.map(|rows| rows.into_iter().map(str::to_string).collect()),
        goal: goal.map(|text| GoalSection {
            text: text.to_string(),
            completed: false,
        }),
        ctx: None,
    }
}

fn prose(heading: Option<&str>, body: &str) -> ProseSection {
    ProseSection {
        heading: heading.map(str::to_string),
        body: body.to_string(),
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

// ── chrome assembly: title / prose / checklist / goal (Decision 3 / 12 / 13) ──
// plan_state and ctx moved to the merged footer (Decision 7 / Decision 12), so
// they must NOT appear in the chrome.

#[test]
fn chrome_classic_order_title_checklist_rows_goal_and_omit_state_and_ctx() {
    let mut s = sections(
        Some("Ship plan mode"),
        Some(vec!["☑ scope it", "☐ build it"]),
        Some("close B"),
    );
    s.plan_state = Some("✍️ Editing plan".to_string());
    s.ctx = Some("ctx 12.3k/200k".to_string());
    let out = s.chrome_classic(false);
    // Blank line between checklist and goal (the classic stand-in for the
    // rich <hr> — Decision 13); no blank line between title and checklist.
    // The goal is its own expandable with the bold Decision 10 prefix.
    assert_eq!(
        out,
        "📋 <b>Ship plan mode</b>\n☑ scope it\n☐ build it\n\n\
         <blockquote expandable><b>🎯 Goal:</b> close B</blockquote>"
    );
    assert!(
        !out.contains("Editing plan"),
        "plan_state stays in the footer"
    );
    assert!(!out.contains("ctx"), "ctx stays in the footer");
}

#[test]
fn chrome_empty_when_all_plan_sections_empty() {
    let mut s = sections(None, None, None);
    // plan_state / ctx set but no title/prose/checklist/goal → no chrome.
    s.plan_state = Some("✍️ Editing plan".to_string());
    s.ctx = Some("ctx 1k/200k".to_string());
    assert!(s.chrome_classic(false).is_empty());
    assert!(s.chrome_rich(false).is_empty());
}

#[test]
fn chrome_escapes_html_in_section_text() {
    let s = sections(Some("a <b> & c"), Some(vec!["☐ x < y"]), None);
    for out in [s.chrome_classic(false), s.chrome_rich(false)] {
        assert!(out.contains("a &lt;b&gt; &amp; c"), "title escaped: {out}");
        assert!(out.contains("☐ x &lt; y"), "checklist row escaped: {out}");
        assert!(!out.contains("a <b> & c"));
    }
}

// ── per-heading prose split (Decision 12) ──

#[test]
fn split_plan_prose_strips_h1_and_cuts_on_top_level_headings() {
    let md = "# The Plan\n\nintro paragraph\n\n## Context\nwhy we do it\n\n## Steps\n1. first\n2. second\n";
    let secs = split_plan_prose(md);
    assert_eq!(
        secs,
        vec![
            prose(None, "intro paragraph"),
            prose(Some("Context"), "why we do it"),
            prose(Some("Steps"), "1. first\n2. second"),
        ]
    );
}

#[test]
fn split_plan_prose_keeps_nested_headings_in_the_body() {
    let secs = split_plan_prose("## Design\n### Option A\ntext a\n### Option B\ntext b");
    assert_eq!(
        secs,
        vec![prose(
            Some("Design"),
            "### Option A\ntext a\n### Option B\ntext b"
        )]
    );
}

#[test]
fn split_plan_prose_ignores_headings_inside_code_fences() {
    let md = "## Setup\n```\n## not a heading\n# neither\n```\ndone";
    let secs = split_plan_prose(md);
    assert_eq!(secs.len(), 1);
    assert_eq!(secs[0].heading.as_deref(), Some("Setup"));
    assert!(secs[0].body.contains("## not a heading"));
}

#[test]
fn split_plan_prose_drops_empty_sections_and_empty_input() {
    assert!(split_plan_prose("").is_empty());
    assert!(split_plan_prose("# Title Only\n\n").is_empty());
    // A heading with nothing under it has nothing to disclose.
    assert_eq!(
        split_plan_prose("## Empty\n\n## Real\ncontent"),
        vec![prose(Some("Real"), "content")]
    );
}

// ── prose rendering: rich details per heading / classic expandables ──

#[test]
fn rich_prose_renders_one_details_per_heading_flush_after_title() {
    let mut s = sections(Some("The Plan"), None, None);
    s.prose = Some(vec![
        prose(None, "intro line"),
        prose(Some("Context"), "why\n\nand how"),
    ]);
    let out = s.chrome_rich(false);
    // Title flush against prose (no spacer), orphan preamble always visible,
    // heading as inline summary, blank body lines dropped on rich.
    assert_eq!(
        out,
        "<p>📋 <b>The Plan</b></p><p>intro line</p>\
         <details><summary>Context</summary><p>why</p><p>and how</p></details>"
    );
}

#[test]
fn rich_prose_then_checklist_then_goal_use_hr_boundaries() {
    let mut s = sections(Some("P"), Some(vec!["☐ a"]), Some("g"));
    s.prose = Some(vec![prose(Some("Ctx"), "body")]);
    let out = s.chrome_rich(false);
    assert_eq!(
        out,
        "<p>📋 <b>P</b></p><details><summary>Ctx</summary><p>body</p></details>\
         <hr><p>☐ a</p><hr><p><b>🎯 Goal:</b> g</p>"
    );
}

#[test]
fn rich_title_and_checklist_without_prose_have_no_hr_between_them() {
    let s = sections(Some("P"), Some(vec!["☐ a", "☐ b"]), None);
    assert_eq!(
        s.chrome_rich(false),
        "<p>📋 <b>P</b></p><p>☐ a</p><p>☐ b</p>"
    );
}

#[test]
fn classic_prose_puts_bold_heading_inside_the_expandable() {
    let mut s = sections(Some("The Plan"), None, None);
    s.prose = Some(vec![prose(Some("Context"), "why we do it")]);
    let out = s.chrome_classic(false);
    // The bold heading is the FIRST line inside the blockquote so the
    // collapsed peek shows it (Decision 12) — never a visible line above it.
    assert_eq!(
        out,
        "📋 <b>The Plan</b>\n<blockquote expandable><b>Context</b>\nwhy we do it</blockquote>"
    );
}

#[test]
fn classic_prose_keeps_blank_lines_and_formats_markdown_body() {
    let mut s = sections(None, None, None);
    s.prose = Some(vec![prose(
        Some("Steps"),
        "### Sub\n- item **bold**\n\nplain `code`",
    )]);
    let out = s.chrome_classic(false);
    assert!(out.contains("<b>Sub</b>"), "nested heading bolded: {out}");
    assert!(out.contains("• item <b>bold</b>"), "list bulleted: {out}");
    assert!(out.contains("\n\n"), "paragraph break kept on classic");
    assert!(
        out.contains("plain <code>code</code>"),
        "inline code: {out}"
    );
}

#[test]
fn prose_body_escapes_html_and_keeps_fenced_code_as_code_lines() {
    let mut s = sections(None, None, None);
    s.prose = Some(vec![prose(
        Some("Danger"),
        "a <b> tag\n```\nlet x = 1;\n```",
    )]);
    let out = s.chrome_rich(false);
    assert!(out.contains("a &lt;b&gt; tag"), "body escaped: {out}");
    assert!(
        out.contains("<code>let x = 1;</code>"),
        "fence as code: {out}"
    );
    assert!(!out.contains("```"), "fence markers stripped: {out}");
}

// ── goal chrome: Decision 10 prefixes + Decision 12 collapse ──

#[test]
fn rich_multi_paragraph_goal_collapses_with_first_paragraph_summary() {
    let s = sections(
        None,
        None,
        Some("ship the release\n\nthen tag it\n\nthen announce"),
    );
    let out = s.chrome_rich(false);
    assert_eq!(
        out,
        "<details><summary><b>🎯 Goal:</b> ship the release</summary>\
         <p>then tag it</p><p>then announce</p></details>"
    );
}

#[test]
fn rich_one_paragraph_goal_stays_plain_always_visible() {
    let s = sections(None, None, Some("ship the release"));
    let out = s.chrome_rich(false);
    assert_eq!(out, "<p><b>🎯 Goal:</b> ship the release</p>");
    assert!(!out.contains("<details"), "one paragraph never collapses");
}

#[test]
fn completed_goal_keeps_target_icon_live_and_swaps_to_check_at_settle() {
    let mut s = sections(None, None, Some("close the audit"));
    s.goal.as_mut().expect("goal set").completed = true;
    // While the turn is still running a completed goal keeps 🎯 (Decision 10).
    assert_eq!(
        s.chrome_rich(false),
        "<p><b>🎯 Goal:</b> close the audit</p>"
    );
    // At settle only the icon swaps; the Goal: word never changes.
    assert_eq!(
        s.chrome_rich(true),
        "<p><b>✅ Goal:</b> close the audit</p>"
    );
    assert_eq!(
        s.chrome_classic(true),
        "<blockquote expandable><b>✅ Goal:</b> close the audit</blockquote>"
    );
}

#[test]
fn active_goal_never_shows_check_even_at_settle() {
    // Settle with the goal still active → 🎯 (Decision 10 rule 5).
    let s = sections(None, None, Some("still going"));
    assert_eq!(s.chrome_rich(true), "<p><b>🎯 Goal:</b> still going</p>");
}

// ── plan-state copy (Decision 7): Editing chrome carries no slash hints ──

#[tokio::test]
async fn plan_state_editing_copy_locked_to_decision_7() {
    use crate::channels::telegram::flow_chrome::load_plan_state_section;
    use crate::config::profile::{home_for_profile, with_profile_home_async};
    use crate::utils::plan_files::{create_design_md, save_plan, set_pre_init_editing};
    use uuid::Uuid;

    let profile = format!("flow-chrome-test-{}", Uuid::new_v4());
    with_profile_home_async(Some(&profile), async {
        let sid = Uuid::new_v4();
        // NoPlan → no state line.
        assert_eq!(load_plan_state_section(sid, true).await.0, None);

        // Pre-init Editing: 📝 Discussing plan, no keyboard, no hints.
        set_pre_init_editing(sid).await.unwrap();
        let (state, _) = load_plan_state_section(sid, true).await;
        assert_eq!(state.as_deref(), Some("📝 Discussing plan"));

        // Post-init Editing: ✍️ Editing plan only — the design prose reads on
        // the flow message and Approve rides the keyboard, so the chrome never
        // teaches /show-plan or /execute (Decision 14).
        let plan = crate::tui::plan::PlanDocument::new(sid, "T".to_string());
        save_plan(&plan).await.unwrap();
        create_design_md(sid, "T").await.unwrap();
        let (state, _) = load_plan_state_section(sid, true).await;
        assert_eq!(state.as_deref(), Some("✍️ Editing plan"));
        let state = state.unwrap();
        assert!(!state.contains("/show-plan"), "no view hint: {state}");
        assert!(!state.contains("/execute"), "no approve hint: {state}");
    })
    .await;
    let _ = std::fs::remove_dir_all(home_for_profile(Some(&profile)));
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
    assert!(out.starts_with(
        "<p><b>🎯 Goal:</b> finish the audit</p><p>&nbsp;</p><details><summary><sub>"
    ));
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

#[test]
fn rich_edit_429_retries_rich_never_falls_back_to_html() {
    // A transient 429 on the rich edit must classify as RateLimited (skip and
    // retry rich next tick), NOT Fallback — the HTML path's 4096-char cap would
    // freeze and split a large block that fits the rich 32K limit (#580). Uses
    // the exact error string the rich API surfaced in the wild.
    use crate::channels::telegram::flow::{RichEditError, classify_rich_edit_error};

    let real = "Telegram rich API error (429 Too Many Requests): Too Many Requests: retry after 33";
    assert_eq!(classify_rich_edit_error(real), RichEditError::RateLimited);
    assert_eq!(
        classify_rich_edit_error("Too Many Requests: retry after 5"),
        RichEditError::RateLimited
    );

    // Unchanged content is a no-op.
    assert_eq!(
        classify_rich_edit_error("Bad Request: message is not modified"),
        RichEditError::NotModified
    );

    // Any other failure still falls back to HTML.
    assert_eq!(
        classify_rich_edit_error("Bad Request: can't parse entities"),
        RichEditError::Fallback
    );
}

#[test]
fn plan_card_renders_title_and_checklist_or_none() {
    // The persistent plan card (#580) renders the plan title + checklist, or
    // None when there is no plan content (the caller removes the card then).
    // The prose parameter (#621) folds the design prose into the card in
    // Editing state; these tests pass None since they cover title/checklist.
    use crate::channels::telegram::plan_card::render_plan_card_html;

    assert_eq!(render_plan_card_html(None, None, None, None), None);
    assert_eq!(render_plan_card_html(Some("   "), None, None, None), None);

    let title_only = render_plan_card_html(Some("Audit changes"), None, None, None).unwrap();
    assert!(title_only.contains("📋") && title_only.contains("Audit changes"));

    let rows = vec!["☑ Task one".to_string(), "☐ Task two".to_string()];
    let full = render_plan_card_html(Some("Audit changes"), Some(&rows), None, None).unwrap();
    assert!(full.contains("Audit changes"));
    assert!(full.contains("☑ Task one") && full.contains("☐ Task two"));

    // Checklist without a title still renders.
    let no_title = render_plan_card_html(None, Some(&rows), None, None).unwrap();
    assert!(no_title.contains("☑ Task one"));

    // HTML in a title is escaped, not injected.
    let escaped = render_plan_card_html(Some("a <b> & c"), None, None, None).unwrap();
    assert!(escaped.contains("&lt;b&gt;") && escaped.contains("&amp;"));
}

#[test]
fn plan_card_renders_per_heading_prose() {
    // #621: the card folds the design prose as per-heading expandable
    // blockquotes with bold headings, the same format chrome_classic uses
    // (ADR 0005 Decision 3), in the locked order title → prose → checklist.
    use crate::channels::telegram::flow_chrome::ProseSection;
    use crate::channels::telegram::plan_card::render_plan_card_html;

    let sections = [
        ProseSection {
            heading: Some("Context".to_string()),
            body: "The problem is X.".to_string(),
        },
        ProseSection {
            heading: Some("Implementation steps".to_string()),
            body: "1. Do A\n2. Do B".to_string(),
        },
    ];

    let with_prose =
        render_plan_card_html(Some("Design plan"), None, Some(&sections), None).unwrap();
    assert!(with_prose.contains("Design plan"));
    assert!(with_prose.contains("<blockquote expandable><b>Context</b>"));
    assert!(with_prose.contains("<blockquote expandable><b>Implementation steps</b>"));
    assert!(with_prose.contains("The problem is X."));

    // Prose rides alongside the checklist in Active too (no state gate),
    // always before the rows.
    let rows = vec!["☑ Task one".to_string()];
    let with_both =
        render_plan_card_html(Some("Design plan"), Some(&rows), Some(&sections), None).unwrap();
    assert!(with_both.contains("Task one"));
    assert!(with_both.contains("<blockquote expandable><b>Context</b>"));
    let prose_pos = with_both.find("Context").unwrap();
    let checklist_pos = with_both.find("Task one").unwrap();
    assert!(prose_pos < checklist_pos);
}

#[test]
fn plan_card_renders_goal_after_checklist() {
    use crate::channels::telegram::flow_chrome::GoalSection;
    use crate::channels::telegram::plan_card::render_plan_card_html;

    let rows = vec!["☐ Task one".to_string(), "☐ Task two".to_string()];

    // Active goal: 🎯 prefix, own collapsed expandable, after the checklist
    // (ADR 0005 Decisions 3 + 10).
    let active = GoalSection {
        text: "Ship v0.3.68 without regressions".to_string(),
        completed: false,
    };
    let html =
        render_plan_card_html(Some("Design plan"), Some(&rows), None, Some(&active)).unwrap();
    assert!(html.contains("<blockquote expandable><b>🎯 Goal:</b>"));
    assert!(html.contains("Ship v0.3.68 without regressions"));
    let checklist_pos = html.find("Task two").unwrap();
    let goal_pos = html.find("🎯 Goal:").unwrap();
    assert!(checklist_pos < goal_pos);

    // Completed goal on the settled card swaps the icon to ✅ (Decision 10).
    let done = GoalSection {
        text: "Ship v0.3.68 without regressions".to_string(),
        completed: true,
    };
    let html_done =
        render_plan_card_html(Some("Design plan"), Some(&rows), None, Some(&done)).unwrap();
    assert!(html_done.contains("<blockquote expandable><b>✅ Goal:</b>"));

    // Goal text is HTML-escaped inside the expandable.
    let evil = GoalSection {
        text: "a <b> & c".to_string(),
        completed: false,
    };
    let html_evil =
        render_plan_card_html(Some("Design plan"), Some(&rows), None, Some(&evil)).unwrap();
    assert!(html_evil.contains("a &lt;b&gt; &amp; c"));
    assert!(!html_evil.contains("a <b> & c"));

    // No goal: nothing renders, same as before.
    let no_goal = render_plan_card_html(Some("Design plan"), Some(&rows), None, None).unwrap();
    assert!(!no_goal.contains("Goal:"));
}

#[test]
fn empty_scaffold_lines_are_hidden_filled_ones_kept() {
    // A checklist plan's unfilled .md template must not render as hollow
    // "Problem: / Target state: / Intent: / 1." lines (#580).
    use crate::channels::telegram::flow_chrome::is_empty_scaffold_line;

    assert!(is_empty_scaffold_line("- **Problem:** "));
    assert!(is_empty_scaffold_line("- **Target state:**"));
    assert!(is_empty_scaffold_line("**Intent:**   "));
    assert!(is_empty_scaffold_line("1. "));
    assert!(is_empty_scaffold_line("  1.  "));

    assert!(!is_empty_scaffold_line(
        "- **Problem:** the gate refuses a ready plan"
    ));
    assert!(!is_empty_scaffold_line("1. Widen the hash to 4 chars"));
    assert!(!is_empty_scaffold_line("plain prose line"));
    assert!(!is_empty_scaffold_line(""));
}

#[test]
fn deliverable_rich_report_surfaces_tables_not_narration() {
    // A substantial report with a table is delivered as its own message; thin
    // narration and trivially-short tables keep folding (#582).
    use crate::channels::telegram::intermediates::is_deliverable_rich_report;

    let report = "## Summary\n\n\
        | Fix | Verdict | Medium Gaps | Low Gaps | Cosmetic |\n\
        |-----|---------|-------------|----------|----------|\n\
        | hashline_edit (#573) | PASS | 4 | 4 | 3 |\n\
        | http_request (#574) | PASS | 3 | 8 | 1 |\n\
        | slash_command (#574) | PASS | 0 | 5 | 0 |\n\
        | TOTAL | PASS | 7 | 17 | 4 |\n\n\
        No data loss, no security issues, no regressions. All three fixes are correct and isolated.";
    assert!(is_deliverable_rich_report(report));

    // No table → stays folded (normal progress narration).
    assert!(!is_deliverable_rich_report(
        "Reading the hashline files now, then I'll run clippy and report the gaps back to you."
    ));
    // A table under the length floor isn't worth its own message.
    assert!(!is_deliverable_rich_report("| a | b |\n|-|-|\n| 1 | 2 |"));
}
