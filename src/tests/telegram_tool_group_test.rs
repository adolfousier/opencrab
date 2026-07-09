//! Tests for the processing-log flow renderers.
//!
//! Classic path: grouped tool calls render as native Telegram HTML using
//! `<blockquote expandable>` (Bot API 7.3+), never `<details>` — Telegram's
//! regular HTML parse mode does not support `<details>`, so that tag leaks
//! into the chat as literal text. Rich path (#420 path A): the rich API's
//! HTML INPUT mode is different — there `<details><summary>` is parsed
//! server-side into a native RichBlockDetails collapsible, so
//! `render_flow_details` emits exactly that wrapper.

use crate::channels::telegram::flow::{
    FlowEntry, FlowHeader, FlowOutcome, extract_status_from_text, flow_header_text,
    humanize_duration, latest_activity_preview, pop_trailing_folded_texts, render_flow_html_with,
};
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

// ── latest_activity_preview: whole human-readable text, JSON/code skip (#481) ──

#[test]
fn preview_uses_whole_human_readable_text_untruncated() {
    // Amendment: the whole intermediary text is the status source — every
    // paragraph, newlines preserved, no 96-char cap.
    let long = "First paragraph of the plan.\nSecond line with more detail.\nThird line that pushes well past the old ninety-six character truncation limit so the whole thing survives.";
    let out = latest_activity_preview(&[
        tline("✅ bash", "cargo test"),
        FlowLine::Text(long.to_string()),
    ]);
    assert_eq!(out.as_deref(), Some(long));
}

#[test]
fn preview_skips_json_last_entry_back_to_narration() {
    // A raw-JSON last entry is not human-readable; the preview walks back to the
    // prior narration instead of showing `{"model": ...}`.
    let out = latest_activity_preview(&[
        FlowLine::Text("Checking the model roster.".to_string()),
        FlowLine::Text("{\"model\": \"deepseek-v4-flash\", \"ok\": true}".to_string()),
    ]);
    assert_eq!(out.as_deref(), Some("Checking the model roster."));
}

#[test]
fn preview_skips_code_block_last_entry() {
    let out = latest_activity_preview(&[
        FlowLine::Text("Here is the fix.".to_string()),
        FlowLine::Text("```rust\nfn main() {}\n```".to_string()),
    ]);
    assert_eq!(out.as_deref(), Some("Here is the fix."));
}

#[test]
fn preview_strips_inline_markdown_markers() {
    let out = latest_activity_preview(&[FlowLine::Text(
        "Calling `grep` then **committing** the *fix*.".to_string(),
    )]);
    assert_eq!(
        out.as_deref(),
        Some("Calling grep then committing the fix.")
    );
}

#[test]
fn preview_keeps_one_word_sentence_but_skips_bare_path() {
    // "Done." is narration (keeps letters); a bare path is raw output, skipped.
    assert_eq!(
        latest_activity_preview(&[FlowLine::Text("Done.".to_string())]).as_deref(),
        Some("Done.")
    );
    let out = latest_activity_preview(&[
        tline("✅ read_file", "handler.rs"),
        FlowLine::Text("src/channels/telegram/flow.rs".to_string()),
    ]);
    assert_eq!(out.as_deref(), Some("✅ read_file handler.rs"));
}

#[test]
fn preview_falls_back_to_tool_when_no_human_readable_text() {
    let out = latest_activity_preview(&[
        tline("✅ read_file", "handler.rs"),
        FlowLine::Text("[1,2,3]".to_string()),
    ]);
    assert_eq!(out.as_deref(), Some("✅ read_file handler.rs"));
}

#[test]
fn preview_is_none_when_empty() {
    assert_eq!(latest_activity_preview(&[]), None);
}

// ── extract_status_from_text: bash line-start # comments (#482) ──

#[test]
fn bash_comment_single_strips_decoration() {
    assert_eq!(
        extract_status_from_text("# --- Setup environment ---\nexport FOO=bar").as_deref(),
        Some("Setup environment")
    );
}

#[test]
fn bash_comment_multiple_join_with_newlines() {
    let cmd = "# Step one\napt-get update\n# Step two\napt-get install foo";
    assert_eq!(
        extract_status_from_text(cmd).as_deref(),
        Some("Step one\nStep two")
    );
}

#[test]
fn bash_comment_ignores_inline_hash_and_shebang() {
    // A shebang and an inline (not line-start) `#` are not status comments.
    let cmd = "#!/bin/bash\ncurl https://x.com/a#frag\necho ok";
    assert_eq!(extract_status_from_text(cmd), None);
}

#[test]
fn bash_comment_none_when_no_comments() {
    assert_eq!(extract_status_from_text("cargo build --release"), None);
}

#[test]
fn preview_uses_bash_comments_when_no_narration() {
    // No human-readable text → priority 2 pulls the bash command's comments.
    let out =
        latest_activity_preview(&[tline("⚙️ bash", "# --- Installing deps ---\nnpm install")]);
    assert_eq!(out.as_deref(), Some("Installing deps"));
}

#[test]
fn preview_narration_beats_bash_comments() {
    // Priority 1 (narration) wins over priority 2 (bash comments).
    let out = latest_activity_preview(&[
        tline("⚙️ bash", "# --- Installing deps ---\nnpm install"),
        FlowLine::Text("Wiring up the new module.".to_string()),
    ]);
    assert_eq!(out.as_deref(), Some("Wiring up the new module."));
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

// ── Header wall-clock duration + settled outcome states (#480) ──

#[test]
fn humanize_duration_precise_then_minutes() {
    // Precise wall-clock seconds under a minute, then "X min Ys" (#480).
    assert_eq!(humanize_duration(0), "0s");
    assert_eq!(humanize_duration(45), "45s");
    assert_eq!(humanize_duration(59), "59s");
    assert_eq!(humanize_duration(60), "1 min 0s");
    assert_eq!(humanize_duration(90), "1 min 30s");
    assert_eq!(humanize_duration(300), "5 min 0s");
    assert_eq!(humanize_duration(3661), "61 min 1s");
}

#[test]
fn flow_header_live_and_settled_formats() {
    // Live: gear + count + status suffix; None → plain count / Processing log.
    assert_eq!(
        flow_header_text(3, &FlowHeader::Live(Some("45s"))),
        "⚙️ 3 tool calls · 45s"
    );
    assert_eq!(flow_header_text(3, &FlowHeader::Live(None)), "3 tool calls");
    assert_eq!(
        flow_header_text(0, &FlowHeader::Live(None)),
        "Processing log"
    );
    // Settled: outcome verb, count-in-parens, duration; count clause dropped
    // when no tools ran.
    assert_eq!(
        flow_header_text(
            12,
            &FlowHeader::Settled {
                icon: "✅",
                verb: "Finished",
                duration: "45s"
            }
        ),
        "✅ Finished (12 tool calls, 45s)"
    );
    assert_eq!(
        flow_header_text(
            0,
            &FlowHeader::Settled {
                icon: "✅",
                verb: "Finished",
                duration: "45s"
            }
        ),
        "✅ Finished (45s)"
    );
}

#[test]
fn flow_outcome_icons_and_verbs() {
    assert_eq!(FlowOutcome::Finished.icon_verb(), ("✅", "Finished"));
    assert_eq!(FlowOutcome::Failed.icon_verb(), ("❌", "Failed"));
    assert_eq!(FlowOutcome::TimedOut.icon_verb(), ("⏱", "Timed out"));
}

#[test]
fn settled_outcome_renders_block_header_over_lone_tool() {
    // A settled outcome always renders the block header (not the live lone-tool
    // one-liner) so the ❌/✅/⏱ badge, count, and duration show (#480).
    let out = render_flow_html_with(
        &[tline("✅ bash", "cargo test")],
        &FlowHeader::Settled {
            icon: "❌",
            verb: "Failed",
            duration: "12s",
        },
    );
    assert!(
        out.starts_with("<blockquote expandable><b>❌ Failed (1 tool calls, 12s)</b>"),
        "settled header: {out}"
    );
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
    // Collapsed by default: plain <details>, never <details open>. The summary
    // carries the latest-activity preview so the collapsed block shows progress
    // even with the body hidden (#405); with no narration it falls back to the
    // most recent tool line.
    assert!(out.starts_with(
        "<details><summary><sub><b>2 tool calls</b> ↳ <i>✅ read file.rs</i></sub></summary>"
    ));
    assert!(out.ends_with("</details>"));
    assert!(!out.contains("<details open"));
    // Each entry is its own <p>: the rich HTML parser ignores raw newlines,
    // so without block-level wrapping the log runs together as one wall.
    assert!(out.contains("<p><b>✅ bash</b> <code>git status</code></p>"));
    assert!(out.contains("<p><b>✅ read</b> <code>file.rs</code></p>"));
}

#[test]
fn details_summary_carries_live_status() {
    let out = render_flow_details(
        &[tline("✅ bash", "x"), tline("⚙️ grep", "pattern")],
        Some("grep · 10s"),
    );
    let summary_end = out.find("</summary>").expect("summary");
    let summary = &out[..summary_end];
    assert!(summary.contains("⚙️ 2 tool calls · grep · 10s"));
    // Summary is wrapped in <sub> for visual de-emphasis (#436).
    assert!(summary.contains("<sub><b>⚙️ 2 tool calls · grep · 10s</b>"));
    // Latest-activity preview rides in the summary (#405): the rich <details>
    // collapses to the summary ALONE, hiding the body, so without the preview
    // the collapsed block shows no progress at all. The #436 removal assumed
    // the body covers it, but the body is collapsed by default.
    assert!(summary.contains("↳ <i>"));
}

#[test]
fn details_collapsed_summary_shows_intermediate_narration() {
    // Regression (#405): the COLLAPSED rich block must surface the latest
    // narration in its summary — the body is hidden by default, so the summary
    // is the only place a user sees mid-turn progress. Narration wins over the
    // trailing tool line as the preview source (#481).
    let out = render_flow_details(
        &[
            FlowLine::Text("Running the test suite".to_string()),
            tline("⚙️ bash", "cargo test"),
        ],
        Some("45s"),
    );
    let summary_end = out.find("</summary>").expect("summary");
    let summary = &out[..summary_end];
    assert!(summary.contains("↳ <i>Running the test suite</i>"));
}

#[test]
fn details_escapes_html_in_tool_context() {
    let out = render_flow_details(&[tline("✅ bash", "a < b"), tline("✅ read", "x.rs")], None);
    assert!(out.contains("<code>a &lt; b</code>"));
}

// ── Latest-activity preview in the collapsed block (#405) ──

#[test]
fn collapsed_preview_prefers_narration_over_tool_line() {
    // #481: the status source is the most recent human-readable narration — the
    // latest thing the agent SAID — even when a tool line follows it (that
    // narration usually describes the tool now running), which reads better
    // than a bare "⚙️ read_file src/agent.rs".
    let out = render_flow_html(
        &[
            FlowLine::Text("Checking how the scheduler resolves the next run".to_string()),
            tline("✅ bash", "grep flow"),
            tline("⚙️ read_file", "src/agent.rs"),
        ],
        None,
    );
    let header_end = out.find('\n').expect("header line");
    let preview_line = out[header_end + 1..].lines().next().expect("preview");
    assert!(
        preview_line.contains("Checking how the scheduler resolves the next run"),
        "preview must show the narration: {preview_line}"
    );
    // Full chronological log still follows for the expanded view.
    assert!(out.contains("<b>⚙️ read_file</b> <code>src/agent.rs</code>"));
}

#[test]
fn preview_keeps_long_text_whole_and_strips_markdown() {
    // #481 amendment: no truncation. Inline markers still stripped so the
    // preview never shows raw ** source.
    let long = format!("**{}**", "x".repeat(200));
    let out = render_flow_html(&[tline("✅ bash", "a"), FlowLine::Text(long)], None);
    let header_end = out.find('\n').unwrap();
    let preview_line = out[header_end + 1..].lines().next().unwrap();
    assert!(!preview_line.contains('…'), "not truncated: {preview_line}");
    assert!(
        preview_line.contains(&"x".repeat(200)),
        "whole text kept: {preview_line}"
    );
    assert!(
        !preview_line.contains("**"),
        "markers stripped: {preview_line}"
    );
}

// ── trailing folded-answer reclaim (#478) ───────────────────────────

#[test]
fn trailing_text_run_pops_joined_in_order() {
    // The incident shape: tool rounds, then TWO trailing answer texts
    // (the second folded after a queued follow-up). Both must come out,
    // in order, so neither stays imprisoned in the block.
    let mut entries = vec![
        FlowEntry::Tool(0),
        FlowEntry::Text("first part of the answer".into()),
        FlowEntry::Text("Already factored in. Standing by.".into()),
    ];
    let reclaimed = pop_trailing_folded_texts(&mut entries).expect("reclaims");
    assert_eq!(
        reclaimed,
        "first part of the answer\n\nAlready factored in. Standing by."
    );
    assert_eq!(entries.len(), 1, "tool entry stays");
    assert!(matches!(entries[0], FlowEntry::Tool(0)));
}

#[test]
fn text_only_flow_pops_everything() {
    let mut entries = vec![
        FlowEntry::Text("the whole answer".into()),
        FlowEntry::Text("plus a follow-up".into()),
    ];
    let reclaimed = pop_trailing_folded_texts(&mut entries).expect("reclaims");
    assert!(reclaimed.starts_with("the whole answer"));
    assert!(entries.is_empty());
}

#[test]
fn tool_last_flow_reclaims_nothing() {
    // Trailing tool call = the answer is in response.content, not folded.
    let mut entries = vec![FlowEntry::Text("narration".into()), FlowEntry::Tool(0)];
    assert!(pop_trailing_folded_texts(&mut entries).is_none());
    assert_eq!(entries.len(), 2, "nothing consumed");
}

// ── folded narration cap keeps the block compact (#489) ─────────────

#[test]
fn long_folded_narration_is_capped_in_the_block() {
    // A verbose CLI narration entry (over the 300-char cap) renders
    // truncated with an ellipsis, so the collapsed block stays small and
    // more tool rounds fit before the 30K rich size freeze.
    let long = "x".repeat(1200);
    let out = render_flow_html(
        &[
            tline("✅ bash", "cargo test"),
            FlowLine::Text(long.clone()),
            FlowLine::Text("all done here now".into()),
            tline("✅ read", "flow.rs"),
        ],
        None,
    );
    assert!(out.contains('…'), "capped body entry ends with an ellipsis");
    // A LATER human text is the latest-activity preview (#487, shown whole),
    // so the long entry is body-only and its full run must NOT appear.
    assert!(!out.contains(&long), "body narration must be truncated");
}
