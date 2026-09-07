//! #1221/#1225: the background-task echo bubble renderer + the
//! session_notify label plumbing. Pure assertions — no bot, no runtime:
//! framing strips (System + notify header), bubble assembly (rich markdown +
//! HTML fallback), truncation discipline (raw text cut BEFORE conversion,
//! wrapper tags stay intact).

use crate::brain::agent::BgTaskMeta;
use crate::channels::telegram::resume::{
    NotifySender, background_task_title, build_bg_echo_bubble, build_bg_receipt_card,
    build_notify_receipt_card, split_bg_echo_parts, split_notify_header, strip_system_framing,
};
use uuid::Uuid;

const SENDER: &str = "6c1c9cb9-8243-4def-abe5-d926d0ca8bed";

#[test]
fn strips_notify_header_and_returns_sender() {
    let ctx = format!("[session-notify from={SENDER}]\n\nhello from the other topic");
    let (sender, body) = split_bg_echo_parts(&ctx);
    assert_eq!(
        sender,
        Some(NotifySender::Session(Uuid::parse_str(SENDER).unwrap()))
    );
    assert_eq!(body, "hello from the other topic");
}

#[test]
fn cli_sender_label_is_carried_verbatim() {
    // #23 (owner amendment "Overridable"): the CLI lane stamps
    // `from=cli:<label>` — no sender session exists, so the label rides the
    // header verbatim and the echo renders it without a session lookup.
    let ctx = "[session-notify from=cli:oc-deploy]\n\nbuild green";
    let (sender, body) = split_bg_echo_parts(ctx);
    assert_eq!(sender, Some(NotifySender::CliTooling("oc-deploy")));
    assert_eq!(body, "build green");
}

#[test]
fn cli_label_survives_surrounding_whitespace() {
    let ctx = "[session-notify from=cli: CI runner ]\n\nbody";
    let (sender, body) = split_notify_header(ctx);
    assert_eq!(sender, Some(NotifySender::CliTooling("CI runner")));
    assert_eq!(body, "body");
}

#[test]
fn empty_cli_label_is_malformed_framing() {
    let ctx = "[session-notify from=cli:]\n\nbody";
    let (sender, body) = split_notify_header(ctx);
    assert_eq!(sender, None, "an empty cli: label is not a sender");
    assert_eq!(body, ctx, "malformed header passes whole text through");
}

#[test]
fn split_notify_header_rejects_malformed_framing() {
    let bad_uuid = "[session-notify from=not-a-uuid]\n\nbody";
    let (sender, body) = split_notify_header(bad_uuid);
    assert_eq!(sender, None);
    assert_eq!(
        body, bad_uuid,
        "malformed header must pass whole text through"
    );
    let no_close = "[session-notify from=6c1c9cb9-8243-4def-abe5-d926d0ca8bed";
    let (sender, body) = split_notify_header(no_close);
    assert_eq!(sender, None);
    assert_eq!(body, no_close);
}

#[test]
fn strips_terminated_system_framing() {
    // Real framing shape (background_tasks.rs): block ends with ']'.
    let ctx = "[System: the background task you started has finished.\nStatus: exit 0]\nreal tail";
    let (sender, body) = split_bg_echo_parts(ctx);
    assert_eq!(sender, None, "background tasks carry no sender");
    assert!(!body.contains("[System:"), "scaffolding must not render");
    assert!(body.contains("Status: exit 0"), "inner content survives");
    assert!(body.contains("real tail"));
}

#[test]
fn system_framing_without_closing_brace_passes_through() {
    let ctx = "[System: task finished]\nsome output";
    let inner = strip_system_framing(ctx);
    assert_eq!(
        inner, ctx,
        "the ']' must terminate the block to be stripped"
    );
}

#[test]
fn strips_system_framing_even_after_notify_header() {
    let ctx =
        format!("[session-notify from={SENDER}]\n\n[System: the push you asked for]\npushed body");
    let (sender, body) = split_bg_echo_parts(&ctx);
    assert!(sender.is_some());
    assert!(
        !body.contains("[System:"),
        "System framing stripped after header"
    );
    assert!(body.contains("pushed body"));
}

#[test]
fn absent_framing_passes_through_untouched() {
    let (sender, body) = split_bg_echo_parts("plain text, no framing");
    assert_eq!(sender, None);
    assert_eq!(body, "plain text, no framing");
}

#[test]
fn bubble_wraps_in_blockquote_with_bold_header() {
    let (markdown, html) = build_bg_echo_bubble("some output", "📨 Ops / Push to session");
    assert!(markdown.contains("📨 Ops / Push to session"));
    assert!(markdown.contains("some output"));
    assert!(html.starts_with("<blockquote expandable>"));
    assert!(html.ends_with("</blockquote>"));
    assert!(html.contains("<b>📨 Ops / Push to session</b>"));
    assert!(html.contains("some output"));
}

#[test]
fn background_task_title_is_preserved() {
    let (_, html) = build_bg_echo_bubble("finished ok", "⚙️ background task result");
    assert!(html.contains("<b>⚙️ background task result</b>"));
}

#[test]
fn rich_markdown_keeps_fences() {
    let ctx = "# Heading\n```rust\nfn main() {}\n```";
    let (markdown, _) = build_bg_echo_bubble(ctx, "📨 Team");
    assert!(markdown.contains("```rust"));
    assert!(markdown.contains("# Heading"));
}

#[test]
fn long_output_is_truncated_before_conversion_and_stays_wellformed() {
    let big = format!("{{}}\n{}", "y".repeat(10_000));
    let (markdown, html) = build_bg_echo_bubble(&big, "⚙️ background task result");
    assert!(markdown.contains("(truncated)"));
    assert!(html.contains("(truncated)"));
    // Truncating raw text first means the wrapper tags can never be cut:
    assert!(html.starts_with("<blockquote expandable>"));
    assert!(html.ends_with("</blockquote>"));
}

#[test]
fn background_title_names_the_task_from_display_text() {
    assert_eq!(
        background_task_title("🔧 background task finished: grep-errors"),
        "🔧 background task finished: grep-errors"
    );
    assert_eq!(
        background_task_title("🔧 background task failed: cleanup-unified"),
        "🔧 background task failed: cleanup-unified"
    );
}

#[test]
fn blank_display_text_falls_back_to_generic_title() {
    assert_eq!(background_task_title("   "), "⚙️ background task result");
    assert_eq!(background_task_title(""), "⚙️ background task result");
}

#[test]
fn overlong_task_label_is_capped_in_title() {
    let long = format!("🔧 background task finished: {}", "x".repeat(500));
    let t = background_task_title(&long);
    assert!(t.chars().count() <= 120, "header must stay readable");
    assert!(t.starts_with("🔧 background task finished:"));
}

#[test]
fn html_fallback_escapes_dynamic_title() {
    let (_, html) = build_bg_echo_bubble("body", "📨 Ops <script> / Push");
    assert!(html.contains("<b>📨 Ops &lt;script&gt; / Push</b>"));
    assert!(!html.contains("<script>"), "title must not inject raw HTML");
}

#[test]
fn bubble_md_and_classic_stay_separate() {
    let (md, classic) = build_bg_echo_bubble("body", "T");
    assert!(
        classic.contains("blockquote expandable"),
        "fallback stays classic-dialect"
    );
    assert!(
        !md.contains('<'),
        "markdown leg stays tag-free for the rich parser"
    );
}

// ---- #15 receipt cards (owner-locked shapes P3f / N4) ----

fn meta(success: bool, label: &str, secs: f32, tail: &str) -> BgTaskMeta {
    BgTaskMeta {
        success,
        label: label.to_string(),
        elapsed_secs: secs,
        tail: tail.to_string(),
    }
}

#[test]
fn bg_receipt_card_matches_the_locked_p3f_shape() {
    let (md, classic) = build_bg_receipt_card(&meta(
        true,
        "gh run watch 33117665576",
        1646.0,
        "line one\nline two",
    ));
    assert!(
        md.starts_with(
            "<details>\n<summary><sub>✅ `gh run watch 33117665576` 🕒 27m 26s</sub></summary>"
        ),
        "summary = icon + monospace roster label + clock + duration, whole line subbed: {md}"
    );
    assert!(
        md.contains("```\nline one\nline two\n```"),
        "body is ONE fenced block with the tail verbatim"
    );
    assert!(md.ends_with("</details>"));
    assert!(!md.contains("exit"), "no exit code / wording in the bubble");
    // Degraded path stays a classic blockquote carrying the same content.
    assert!(classic.starts_with("<blockquote expandable>"));
    assert!(classic.contains("gh run watch 33117665576"));
    assert!(classic.contains("line one"));
}

#[test]
fn bg_receipt_card_failure_uses_the_cross_icon() {
    let (md, _) = build_bg_receipt_card(&meta(false, "cargo test", 3.0, "boom"));
    assert!(md.starts_with("<details>\n<summary><sub>❌ `cargo test` 🕒 3s</sub></summary>"));
}

#[test]
fn bg_receipt_card_strips_backticks_from_the_label() {
    let (md, _) = build_bg_receipt_card(&meta(true, "cat `file`.md", 1.0, "ok"));
    assert!(
        md.contains("`cat file.md`"),
        "label backticks stripped so the code span stays intact: {md}"
    );
}

#[test]
fn bg_receipt_card_fence_outgrows_backtick_runs_in_the_tail() {
    let tail = "look:\n```\nnested fence\n```\ndone";
    let (md, _) = build_bg_receipt_card(&meta(true, "cat README.md", 2.0, tail));
    assert!(
        md.contains("````\nlook:"),
        "fence grows past the tail's longest backtick run"
    );
}

#[test]
fn bg_receipt_card_empty_tail_is_a_flat_one_liner() {
    // A whitespace-only tail leaves nothing inside the <details> wrapper —
    // the rich API rejects the card with RICH_MESSAGE_EMPTY and the outbox
    // fallback escapes the literal wrapper tags into the chat. The guard
    // must emit a flat card with no wrapper on either leg.
    for tail in ["", "   ", "\n\t"] {
        let (md, classic) = build_bg_receipt_card(&meta(true, "gh run watch 1", 61.0, tail));
        assert!(!md.contains("<details>"), "no wrapper to reject: {md}");
        assert!(
            !classic.contains("<details>"),
            "no wrapper to leak: {classic}"
        );
        assert!(
            md.starts_with("✅ `gh run watch 1` 🕒 "),
            "flat markdown card: {md}"
        );
        assert!(!md.contains('\n'), "one line only: {md}");
        assert!(
            classic.starts_with("<b>✅ gh run watch 1 🕒 "),
            "flat classic card: {classic}"
        );
    }
}

#[test]
fn bg_receipt_card_empty_tail_escapes_the_label_on_the_classic_leg() {
    let (md, classic) = build_bg_receipt_card(&meta(false, "grep <b>", 1.0, ""));
    assert!(
        classic.contains("&lt;b&gt;"),
        "classic leg escapes the label: {classic}"
    );
    assert!(
        !classic.contains("<b>grep"),
        "label cannot open a tag: {classic}"
    );
    assert!(
        md.starts_with("❌ `grep <b>` 🕒 "),
        "markdown leg keeps the label verbatim: {md}"
    );
}

#[test]
fn notify_receipt_card_matches_the_locked_n4_shape() {
    let body = "RECEIPT CONTRACT DELIVERED — swap verified, all three clauses journal-anchored.\n\n\
                | Clause | Anchor |\n|---|---|\n| Build | run 1 |";
    let (md, classic) = build_notify_receipt_card("Compiler", body);
    assert!(
        md.starts_with(
            "<details>\n<summary><sub>📨 From <b>Compiler</b>: RECEIPT CONTRACT DELIVERED — swap \
             verified, a…</sub></summary>"
        ),
        "summary = 📨 + From + bold sender + colon + 45-char first-line preview, whole line subbed: {md}"
    );
    assert!(
        md.contains("|---|---|"),
        "markdown body keeps pipe tables native for the rich parser"
    );
    assert!(!md.contains("```"), "notify body is never fenced");
    assert!(md.ends_with("</details>"));
    assert!(classic.starts_with("<blockquote expandable>"));
    assert!(classic.contains("Compiler"));
}

#[test]
fn notify_receipt_card_sanitizes_angle_brackets_in_sender() {
    let (md, _) = build_notify_receipt_card("Ops <script>", "body line");
    assert!(
        md.contains("<b>Ops ‹script›</b>: body line"),
        "angle brackets neutralized so the sender can't open a tag: {md}"
    );
    assert!(!md.contains("<script>"));
}

#[test]
fn notify_receipt_card_empty_body_is_a_flat_one_liner() {
    // Same defect class as the bg card: a whitespace-only body leaves an
    // empty card inside the <details> wrapper (RICH_MESSAGE_EMPTY). The
    // guard must emit a flat card with no wrapper on either leg.
    for body in ["", "  \n\t "] {
        let (md, classic) = build_notify_receipt_card("Compiler", body);
        assert!(!md.contains("<details>"), "no wrapper to reject: {md}");
        assert!(
            !classic.contains("<details>"),
            "no wrapper to leak: {classic}"
        );
        assert_eq!(md, "📨 From **Compiler**", "flat markdown card");
        assert_eq!(classic, "📨 From <b>Compiler</b>", "flat classic card");
    }
}

#[test]
fn notify_preview_truncates_the_first_line_only() {
    let body = format!("{}\nsecond line stays in the body", "x".repeat(80));
    let (md, _) = build_notify_receipt_card("Worker", &body);
    let preview = format!("{}…", "x".repeat(45));
    assert!(
        md.contains(&format!(": {preview}</sub>")),
        "preview = first line truncated to 45 chars + ellipsis: {md}"
    );
    assert!(
        md.contains("second line stays in the body"),
        "the full body survives inside the fold"
    );
}

/// Parser-level end-to-end for the #15 receipt-card envelope — the wire
/// shape the #1259 outbox architecture actually sends. The #1234
/// markdown-ladder variant and its parse test are gone with the ladder;
/// the contract they proved survives here, retargeted at the production
/// envelope: the card must parse into ONE Details block whose body keeps
/// a NATIVE table for the server-side rich route.
#[test]
fn notify_receipt_card_parses_to_details_with_native_table_inside() {
    use crate::channels::telegram::rich::ast::Block;
    use crate::channels::telegram::rich::parse::parse_markdown;

    let body = "| a | b |\n|---|---|\n| 1 | 2 |";
    let (md, _) = build_notify_receipt_card("Compiler", body);
    let blocks = parse_markdown(&md);
    match blocks.as_slice() {
        [Block::Details { blocks, .. }] => {
            assert!(
                blocks.iter().any(|b| matches!(b, Block::Table { .. })),
                "card body keeps a native table block, got {blocks:?}"
            );
        }
        other => panic!("card must parse as one Details block, got {other:?}"),
    }
}
