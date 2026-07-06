//! Tests for the processing-log flow renderers.
//!
//! Classic path: grouped tool calls render as native Telegram HTML using
//! `<blockquote expandable>` (Bot API 7.3+), never `<details>` — Telegram's
//! regular HTML parse mode does not support `<details>`, so that tag leaks
//! into the chat as literal text. Rich path (#420 path A): the rich API's
//! HTML INPUT mode is different — there `<details><summary>` is parsed
//! server-side into a native RichBlockDetails collapsible, so
//! `render_flow_details` emits exactly that wrapper.

use crate::channels::telegram::handler::{
    FlowLine, folded_duplicates_final, humanize_elapsed, render_flow_details, render_flow_html,
    render_flow_rich,
};

fn tline(label: &str, context: &str) -> FlowLine {
    FlowLine::Tool {
        label: label.to_string(),
        context: context.to_string(),
    }
}

#[test]
fn empty_group_renders_nothing() {
    assert_eq!(render_flow_html(&[], None), "");
}

#[test]
fn single_tool_renders_plain_line_without_blockquote() {
    let out = render_flow_html(&[tline("✅ bash", "git status")], None);
    assert_eq!(out, "<b>✅ bash</b> <code>git status</code>");
    assert!(!out.contains("<blockquote"));
}

#[test]
fn single_tool_without_context_omits_trailing_space() {
    let out = render_flow_html(&[tline("⚙️ web_search", "")], None);
    assert_eq!(out, "<b>⚙️ web_search</b>");
}

#[test]
fn multiple_tools_render_expandable_blockquote() {
    let out = render_flow_html(
        &[
            tline("✅ bash", "cargo fmt"),
            tline("✅ read_file", "handler.rs"),
            tline("❌ grep", "pattern"),
        ],
        None,
    );
    assert!(out.starts_with("<blockquote expandable><b>3 tool calls</b>\n"));
    assert!(out.ends_with("</blockquote>"));
    assert!(out.contains("<b>✅ bash</b> <code>cargo fmt</code>"));
    assert!(out.contains("<b>✅ read_file</b> <code>handler.rs</code>"));
    assert!(out.contains("<b>❌ grep</b> <code>pattern</code>"));
}

#[test]
fn blocks_are_separated_by_blank_lines() {
    let out = render_flow_html(
        &[
            tline("✅ bash", "cargo fmt"),
            FlowLine::Text("Reformatted three files.".to_string()),
            tline("✅ read_file", "handler.rs"),
        ],
        None,
    );
    // Blank line after the header and between every block, so the collapsed
    // log reads as separated entries instead of a cramped wall.
    assert!(out.starts_with("<blockquote expandable><b>2 tool calls</b>\n↳ <i>"));
    assert!(out.contains("<b>✅ bash</b> <code>cargo fmt</code>\n\nReformatted three files."));
    assert!(
        out.contains("Reformatted three files.\n\n<b>✅ read_file</b> <code>handler.rs</code>")
    );
}

#[test]
fn tool_context_renders_as_monospace() {
    // Paths / commands / queries read as code, not prose, in the expanded block.
    let out = render_flow_html(
        &[
            tline("✅ read", "src/channels/telegram/handler.rs"),
            tline("✅ bash", "cargo clippy --all-features"),
        ],
        None,
    );
    assert!(out.contains("<b>✅ read</b> <code>src/channels/telegram/handler.rs</code>"));
    assert!(out.contains("<b>✅ bash</b> <code>cargo clippy --all-features</code>"));
}

#[test]
fn intermediate_text_renders_inline_markdown() {
    // Narration folded into the block gets the same inline markdown as the final
    // completion below it: `code` spans, **bold**, *italic* render, not raw.
    let out = render_flow_html(
        &[
            tline("✅ bash", "grep foo"),
            FlowLine::Text("Calling `analyze_image` then **committing** the *fix*.".to_string()),
        ],
        None,
    );
    assert!(out.contains("<code>analyze_image</code>"));
    assert!(out.contains("<b>committing</b>"));
    assert!(out.contains("<i>fix</i>"));
    // No raw markdown markers survive — the body renders them and the
    // latest-activity preview strips them (#405).
    assert!(!out.contains("`analyze_image`"));
    assert!(!out.contains("**committing**"));
}

#[test]
fn never_emits_details_tags() {
    let out = render_flow_html(&[tline("✅ a", "x"), tline("✅ b", "y")], None);
    assert!(!out.contains("<details>"));
    assert!(!out.contains("<summary>"));
}

#[test]
fn escapes_html_in_labels_and_context() {
    let out = render_flow_html(
        &[
            tline("✅ bash", "grep '<details>' & \"stuff\""),
            tline("✅ edit_file", "a < b > c"),
        ],
        None,
    );
    assert!(out.contains("grep '&lt;details&gt;' &amp; \"stuff\""));
    assert!(out.contains("a &lt; b &gt; c"));
    // No raw angle brackets from content survive outside our own tags
    assert!(!out.contains("'<details>'"));
}

// ── Mixed processing-log flow (tool calls + intermediate text) — #300 ──

#[test]
fn tool_plus_text_folds_into_one_blockquote() {
    let out = render_flow_html(
        &[
            tline("✅ bash", "git status"),
            FlowLine::Text("Checked the tree, all clean.".to_string()),
            tline("✅ read_file", "handler.rs"),
        ],
        None,
    );
    assert!(out.starts_with("<blockquote expandable><b>2 tool calls</b>\n"));
    assert!(out.ends_with("</blockquote>"));
    assert!(out.contains("<b>✅ bash</b> <code>git status</code>"));
    assert!(out.contains("Checked the tree, all clean."));
    assert!(out.contains("<b>✅ read_file</b> <code>handler.rs</code>"));
    assert!(!out.contains("<details>"));
}

#[test]
fn text_only_flow_uses_processing_log_header() {
    let out = render_flow_html(&[FlowLine::Text("Switching provider…".to_string())], None);
    assert!(out.starts_with("<blockquote expandable><b>Processing log</b>\n"));
    assert!(out.contains("Switching provider…"));
    assert!(!out.contains("tool calls"));
}

#[test]
fn intermediate_text_is_html_escaped_in_flow() {
    let out = render_flow_html(
        &[
            tline("✅ bash", "echo hi"),
            FlowLine::Text("result: <b>bold</b> & <script>alert(1)</script>".to_string()),
        ],
        None,
    );
    assert!(out.contains("&lt;b&gt;bold&lt;/b&gt; &amp; &lt;script&gt;"));
    // The injected tags must never survive as live HTML.
    assert!(!out.contains("<script>"));
}

#[test]
fn blank_text_entries_are_dropped() {
    let out = render_flow_html(
        &[tline("✅ bash", "x"), FlowLine::Text("   ".to_string())],
        None,
    );
    // A lone tool with only blank text collapses to the one-liner.
    assert_eq!(out, "<b>✅ bash</b> <code>x</code>");
}

#[test]
fn empty_flow_renders_nothing() {
    assert_eq!(render_flow_html(&[], None), "");
}

// ── folded_duplicates_final: block dedup against the final answer ──
// A streamed copy of the final answer can land folded in the collapsed block.
// On API providers the answer also comes back in response.content, so it must
// be dropped from the block or it renders twice (once folded, once as the
// completion). Streaming often folds only a truncated head, so the check must
// catch a PREFIX, not just an exact match.

#[test]
fn folded_dup_matches_exact_final() {
    let answer = "The rebuild finished and the binary is swapped in.";
    assert!(folded_duplicates_final(answer, answer));
}

#[test]
fn folded_dup_matches_truncated_prefix() {
    // The block captured only the streamed head, cut off mid-sentence.
    let folded = "Yes, Adolfo. After you told me to search for the tool, I";
    let final_text =
        "Yes, Adolfo. After you told me to search for the tool, I found it and used it.";
    assert!(folded_duplicates_final(folded, final_text));
}

#[test]
fn folded_dup_ignores_whitespace_differences() {
    let folded = "line one\n\n  line two three four five";
    let final_text = "line one line two three four five and the rest of the answer here";
    assert!(folded_duplicates_final(folded, final_text));
}

#[test]
fn folded_dup_rejects_distinct_narration() {
    // A genuine mid-turn narration line that isn't the final answer.
    let folded = "Let me trace the delivery path first.";
    let final_text = "The root cause is an exact-equality dedup that misses a truncated prefix.";
    assert!(!folded_duplicates_final(folded, final_text));
}

#[test]
fn folded_dup_rejects_short_shared_opening() {
    // Too short an overlap to be sure it's the answer, not a coincidental start.
    let folded = "Done.";
    let final_text = "Done. Here is the full breakdown of everything that changed this turn.";
    assert!(!folded_duplicates_final(folded, final_text));
}

#[test]
fn folded_dup_exact_short_answer_matches() {
    // #316: a short final answer folded verbatim used to slip under the
    // 20-char prefix guard and render twice (in the block AND as the
    // completion). Exact equality is a duplicate at any length.
    assert!(folded_duplicates_final("Dropped it.", "Dropped it."));
}

#[test]
fn folded_dup_exact_short_with_whitespace_matches() {
    assert!(folded_duplicates_final("  Dropped   it.\n", "Dropped it."));
}

#[test]
fn folded_dup_empty_sides_never_match() {
    assert!(!folded_duplicates_final("", ""));
    assert!(!folded_duplicates_final("", "Dropped it."));
    assert!(!folded_duplicates_final("Dropped it.", ""));
}

// ── Live status in the block header — single progress surface (#360) ──
// While the turn runs, the open block's header carries the rolling status
// ("N tool calls · read_file · 45s") so no standalone ticker message exists
// alongside the block. When the final response lands the status is cleared
// and the header settles back to the plain count.

#[test]
fn live_status_rides_in_blockquote_header() {
    let out = render_flow_html(
        &[
            tline("✅ bash", "cargo fmt"),
            tline("⚙️ read_file", "handler.rs"),
        ],
        Some("read_file · 45s"),
    );
    assert!(out.starts_with("<blockquote expandable><b>⚙️ 2 tool calls · read_file · 45s</b>\n"));
    assert!(out.ends_with("</blockquote>"));
}

#[test]
fn no_status_renders_plain_settled_header() {
    let out = render_flow_html(
        &[
            tline("✅ bash", "cargo fmt"),
            tline("✅ read_file", "handler.rs"),
        ],
        None,
    );
    assert!(out.starts_with("<blockquote expandable><b>2 tool calls</b>\n"));
    assert!(!out.contains("⚙️ 2 tool calls"));
}

#[test]
fn live_status_on_text_only_flow_uses_processing_log_header() {
    let out = render_flow_html(
        &[FlowLine::Text("Looking into it.".to_string())],
        Some("15s"),
    );
    assert!(out.starts_with("<blockquote expandable><b>⚙️ Processing log · 15s</b>\n"));
}

#[test]
fn live_status_appends_to_single_tool_line() {
    // A lone tool call renders as a plain line (no blockquote); the status
    // still rides on it so the single surface shows progress from call one.
    let out = render_flow_html(&[tline("⚙️ bash", "git status")], Some("bash · 5s"));
    assert_eq!(out, "<b>⚙️ bash</b> <code>git status</code> · bash · 5s");
    assert!(!out.contains("<blockquote"));
}

#[test]
fn humanize_elapsed_snaps_to_five_second_steps() {
    // 5s snapping keeps header edits at most one per ~5s, not one per tick.
    assert_eq!(humanize_elapsed(0), "0s");
    assert_eq!(humanize_elapsed(4), "0s");
    assert_eq!(humanize_elapsed(7), "5s");
    assert_eq!(humanize_elapsed(59), "55s");
    assert_eq!(humanize_elapsed(61), "1m 0s");
    assert_eq!(humanize_elapsed(93), "1m 30s");
    assert_eq!(humanize_elapsed(3601), "60m 0s");
}

// ── Rich API flow rendering tests (#393) ─────────────────────────────────────

#[test]
fn rich_empty_group_renders_nothing() {
    assert_eq!(render_flow_rich(&[], None), "");
}

#[test]
fn rich_single_tool_renders_plain_line() {
    let out = render_flow_rich(&[tline("✅ bash", "git status")], None);
    assert_eq!(out, "**✅ bash** `git status`");
    // Single tool: no blockquote wrapping, just bold label + context
    assert!(!out.contains(">"));
}

#[test]
fn rich_multiple_tools_render_markdown_header() {
    let out = render_flow_rich(
        &[tline("✅ bash", "git status"), tline("✅ read", "file.rs")],
        None,
    );
    // Preview is plain text (no italics): tool contexts routinely carry
    // underscores, which would shred the markdown italics markers.
    assert!(out.starts_with("**2 tool calls**\n↳ "));
    assert!(out.contains("**✅ bash** `git status`"));
    assert!(out.contains("**✅ read** `file.rs`"));
}

#[test]
fn rich_live_status_in_header() {
    let out = render_flow_rich(
        &[tline("✅ bash", "x"), tline("⚙️ grep", "pattern")],
        Some("grep · 10s"),
    );
    assert!(out.starts_with("**⚙️ 2 tool calls · grep · 10s**\n↳ "));
}

#[test]
fn rich_single_tool_live_status_appends() {
    let out = render_flow_rich(&[tline("⚙️ bash", "git status")], Some("bash · 5s"));
    assert_eq!(out, "**⚙️ bash** `git status` · bash · 5s");
}

// ── Rich-API details flow rendering (#420 path A) ──

#[test]
fn details_empty_group_renders_nothing() {
    assert_eq!(render_flow_details(&[], None), "");
}

#[test]
fn details_single_tool_renders_plain_line() {
    // Lone tool line stays plain (mirrors #296), same as the HTML renderer.
    let out = render_flow_details(&[tline("✅ bash", "git status")], None);
    assert_eq!(out, "<b>✅ bash</b> <code>git status</code>");
    assert!(!out.contains("<details"));
}

#[test]
fn details_multiple_tools_wrap_in_collapsed_details() {
    let out = render_flow_details(
        &[tline("✅ bash", "git status"), tline("✅ read", "file.rs")],
        None,
    );
    // Collapsed by default: plain <details>, never <details open>.
    assert!(out.starts_with("<details><summary><b>2 tool calls</b> ↳ "));
    assert!(out.ends_with("</details>"));
    assert!(!out.contains("<details open"));
    // Each entry is its own <p>: the rich HTML parser ignores raw newlines,
    // so without block-level wrapping the log runs together as one wall.
    assert!(out.contains("<p><b>✅ bash</b> <code>git status</code></p>"));
    assert!(out.contains("<p><b>✅ read</b> <code>file.rs</code></p>"));
}

#[test]
fn details_summary_carries_live_status_and_latest_preview() {
    let out = render_flow_details(
        &[tline("✅ bash", "x"), tline("⚙️ grep", "pattern")],
        Some("grep · 10s"),
    );
    let summary_end = out.find("</summary>").expect("summary");
    let summary = &out[..summary_end];
    assert!(summary.contains("⚙️ 2 tool calls · grep · 10s"));
    // Latest-activity preview (#405) rides in the summary so the COLLAPSED
    // block shows what is happening now.
    assert!(summary.contains("↳ ⚙️ grep pattern"));
}

#[test]
fn details_escapes_html_in_tool_context() {
    let out = render_flow_details(&[tline("✅ bash", "a < b"), tline("✅ read", "x.rs")], None);
    assert!(out.contains("<code>a &lt; b</code>"));
}

// ── Latest-activity preview in the collapsed block (#405) ──

#[test]
fn collapsed_preview_shows_latest_entry_not_first() {
    // Telegram's collapsed blockquote shows header + first content line, so
    // the newest activity must ride directly under the header.
    let out = render_flow_html(
        &[
            FlowLine::Text("Compris. Laisse-moi d'abord trouver le flow agent".to_string()),
            tline("✅ bash", "grep flow"),
            tline("⚙️ read_file", "src/agent.rs"),
        ],
        None,
    );
    let header_end = out.find('\n').expect("header line");
    let preview_line = out[header_end + 1..].lines().next().expect("preview");
    assert!(
        preview_line.contains("read_file") && preview_line.contains("src/agent.rs"),
        "preview must show the LATEST entry: {preview_line}"
    );
    assert!(
        !preview_line.contains("Compris"),
        "first entry must not be pinned: {preview_line}"
    );
    // Full chronological log still follows for the expanded view.
    assert!(out.contains("Compris. Laisse-moi"));
}

#[test]
fn preview_truncates_and_strips_markdown() {
    let long = format!("**{}**", "x".repeat(200));
    let out = render_flow_html(&[tline("✅ bash", "a"), FlowLine::Text(long)], None);
    let header_end = out.find('\n').unwrap();
    let preview_line = out[header_end + 1..].lines().next().unwrap();
    assert!(preview_line.contains('…'), "truncated: {preview_line}");
    assert!(
        !preview_line.contains("**"),
        "markers stripped: {preview_line}"
    );
}
