//! #1221/#1225: the background-task echo bubble renderer + the
//! session_notify label plumbing. Pure assertions — no bot, no runtime:
//! framing strips (System + notify header), bubble assembly (rich markdown +
//! HTML fallback), truncation discipline (raw text cut BEFORE conversion,
//! wrapper tags stay intact).

use crate::brain::agent::BgTaskMeta;
use crate::channels::telegram::resume::{
    BubbleWire, NotifySender, background_task_title, build_bg_echo_bubble, build_bg_receipt_card,
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
    let (wire, html) = build_bg_echo_bubble("some output", "📨 Ops / Push to session");
    let markdown = match wire {
        BubbleWire::Markdown(md) => md,
        _ => panic!("echo bubble rides the markdown outbox wire (#38)"),
    };
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
    let (wire, _) = build_bg_echo_bubble(ctx, "📨 Team");
    let markdown = match wire {
        BubbleWire::Markdown(md) => md,
        _ => panic!("echo bubble rides the markdown outbox wire (#38)"),
    };
    assert!(markdown.contains("```rust"));
    assert!(markdown.contains("# Heading"));
}

#[test]
fn long_output_is_truncated_before_conversion_and_stays_wellformed() {
    let big = format!("{{}}\n{}", "y".repeat(10_000));
    let (wire, html) = build_bg_echo_bubble(&big, "⚙️ background task result");
    let markdown = match wire {
        BubbleWire::Markdown(md) => md,
        _ => panic!("echo bubble rides the markdown outbox wire (#38)"),
    };
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
    let (wire, classic) = build_bg_echo_bubble("body", "T");
    let md = match wire {
        BubbleWire::Markdown(md) => md,
        _ => panic!("echo bubble rides the markdown outbox wire (#38)"),
    };
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
    let (wire, classic) = build_bg_receipt_card(&meta(
        true,
        "gh run watch 33117665576",
        1646.0,
        "line one\nline two",
    ));
    let rich_html = match wire {
        BubbleWire::Html(html) => html,
        _ => panic!("bg card rides the HTML rich wire (#38)"),
    };
    assert!(
        rich_html.starts_with(
            "<details><summary><sub>✅ <code>gh run watch 33117665576</code> 🕒 27m 26s</sub></summary>"
        ),
        "summary = icon + monospace roster label + clock + duration, whole line subbed: {rich_html}"
    );
    assert!(
        rich_html.contains("<pre>line one\nline two</pre>"),
        "body is the output tail verbatim inside <pre>: {rich_html}"
    );
    assert!(rich_html.ends_with("</details>"));
    assert!(
        !rich_html.contains("exit"),
        "no exit code / wording in the bubble"
    );
    // Degraded path stays a classic blockquote carrying the same content.
    assert!(classic.starts_with("<blockquote expandable>"));
    assert!(classic.contains("gh run watch 33117665576"));
    assert!(classic.contains("line one"));
}

#[test]
fn bg_receipt_card_failure_uses_the_cross_icon() {
    let (wire, _) = build_bg_receipt_card(&meta(false, "cargo test", 3.0, "boom"));
    let md = match wire {
        BubbleWire::Html(html) => html,
        _ => panic!("bg card rides the HTML rich wire (#38)"),
    };
    assert!(
        md.starts_with("<details><summary><sub>❌ <code>cargo test</code> 🕒 3s</sub></summary>")
    );
}

#[test]
fn bg_receipt_card_strips_backticks_from_the_label() {
    let (wire, _) = build_bg_receipt_card(&meta(true, "cat `file`.md", 1.0, "ok"));
    let md = match wire {
        BubbleWire::Html(html) => html,
        _ => panic!("bg card rides the HTML rich wire (#38)"),
    };
    assert!(
        md.contains("<code>cat file.md</code>"),
        "label backticks stripped so the code span stays intact: {md}"
    );
}

#[test]
fn bg_receipt_card_fence_outgrows_backtick_runs_in_the_tail() {
    let tail = "look:\n```\nnested fence\n```\ndone";
    let (wire, classic) = build_bg_receipt_card(&meta(true, "cat README.md", 2.0, tail));
    // #38: the HTML rich leg carries the tail inside <pre> — containment
    // comes from the tag, backtick runs are inert there.
    let md = match wire {
        BubbleWire::Html(html) => html,
        _ => panic!("bg card rides the HTML rich wire (#38)"),
    };
    assert!(
        md.contains("<pre>look:\n```\nnested fence\n```\ndone</pre>"),
        "tail rides <pre> verbatim on the HTML leg: {md}"
    );
    // The classic leg keeps the fence arms-race: receipt_fence still grows
    // past the tail's longest backtick run. markdown_to_html renders that
    // fenced tail as code block(s) with the tail text preserved (inner
    // fence runs split the block on this leg — pre-existing converter
    // behavior, display-only).
    assert!(
        classic.contains("<pre><code>"),
        "classic leg renders the tail as code block(s): {classic}"
    );
    assert!(
        classic.contains("nested fence"),
        "tail content survives the classic leg: {classic}"
    );
}

// ---- #38: empty-body guard + hostile-tail escaping ----

#[test]
fn bg_receipt_card_empty_tail_drops_the_wrapper() {
    // Regression #38: a whitespace-only tail left nothing inside the
    // <details> wrapper and Telegram rejected the card with 400
    // RICH_MESSAGE_EMPTY; the outbox fallback then re-escaped the wrapper
    // into visible text (the 2026-08-29 leak). Guard: flat one-line card,
    // no wrapper on either leg.
    for tail in ["", " ", "\n  \n"] {
        let (wire, classic) =
            build_bg_receipt_card(&meta(true, "gh run watch 33279712496", 467.0, tail));
        let md = match wire {
            BubbleWire::Markdown(md) => md,
            _ => panic!("empty tail must ride the flat markdown wire (#38 guard)"),
        };
        assert!(
            !md.contains("<details>"),
            "no wrapper on the flat card: {md}"
        );
        assert!(
            !classic.contains("<details>"),
            "no wrapper on the fallback: {classic}"
        );
        assert!(md.contains("`gh run watch 33279712496`"));
        assert!(md.contains("🕒 7m 47s"));
    }
}

#[test]
fn bg_receipt_card_escapes_hostile_tail_in_pre() {
    let (wire, _) = build_bg_receipt_card(&meta(
        true,
        "grep '<b>'",
        1.0,
        "a < b & <script>alert(1)</script>",
    ));
    let md = match wire {
        BubbleWire::Html(html) => html,
        _ => panic!("bg card rides the HTML rich wire (#38)"),
    };
    assert!(
        md.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
        "tail is escaped inside <pre>: {md}"
    );
    assert!(!md.contains("<script>"), "raw HTML must not inject: {md}");
    assert!(
        md.contains("<code>grep &lt;b&gt;</code>"),
        "label is escaped too: {md}"
    );
}

#[tokio::test]
async fn notify_receipt_card_matches_the_locked_n4_shape() {
    let body = "RECEIPT CONTRACT DELIVERED — swap verified, all three clauses journal-anchored.\n\n\
                | Clause | Anchor |\n|---|---|\n| Build | run 1 |";
    let (wire, classic) = build_notify_receipt_card("Compiler", body).await;
    let md = match wire {
        BubbleWire::Html(html) => html,
        _ => panic!("notify card rides the HTML rich wire (#38)"),
    };
    assert!(
        md.starts_with(
            "<details><summary><sub>📨 From <b>Compiler</b>: RECEIPT CONTRACT DELIVERED — swap \
             verified, a…</sub></summary>"
        ),
        "summary = 📨 + From + bold sender + colon + 45-char first-line preview, whole line subbed: {md}"
    );
    // Body rendered from markdown on the HTML leg (#38): the table renders
    // as HTML (grid/key-value/cards — any table shape carries the content).
    assert!(
        md.contains("<table") || md.contains("<pre>"),
        "table body renders on the rich leg: {md}"
    );
    assert!(md.contains("Build"), "table content survives: {md}");
    assert!(!md.contains("```"), "notify body is never fenced");
    assert!(md.ends_with("</details>"));
    assert!(classic.starts_with("<blockquote expandable>"));
    assert!(classic.contains("Compiler"));
}

#[tokio::test]
async fn notify_receipt_card_sanitizes_angle_brackets_in_sender() {
    let (wire, _) = build_notify_receipt_card("Ops <script>", "body line").await;
    let md = match wire {
        BubbleWire::Html(html) => html,
        _ => panic!("notify card rides the HTML rich wire (#38)"),
    };
    assert!(
        md.contains("<b>Ops ‹script›</b>: body line"),
        "angle brackets neutralized so the sender can't open a tag: {md}"
    );
    assert!(!md.contains("<script>"));
}

#[tokio::test]
async fn notify_preview_truncates_the_first_line_only() {
    let body = format!("{}\nsecond line stays in the body", "x".repeat(80));
    let (wire, _) = build_notify_receipt_card("Worker", &body).await;
    let md = match wire {
        BubbleWire::Html(html) => html,
        _ => panic!("notify card rides the HTML rich wire (#38)"),
    };
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

#[tokio::test]
async fn notify_receipt_card_empty_body_drops_the_wrapper() {
    // #38: whitespace-only notify body = empty card inside the wrapper =
    // 400 RICH_MESSAGE_EMPTY (same defect class as the bg card). Guard:
    // flat one-line card, no wrapper on either leg.
    for body in ["", "  \n"] {
        let (wire, classic) = build_notify_receipt_card("Compiler", body).await;
        let md = match wire {
            BubbleWire::Markdown(md) => md,
            _ => panic!("empty body must ride the flat markdown wire (#38 guard)"),
        };
        assert!(
            !md.contains("<details>"),
            "no wrapper on the flat card: {md}"
        );
        assert!(
            !classic.contains("<details>"),
            "no wrapper on the fallback: {classic}"
        );
        assert!(md.contains("📨 From **Compiler**"));
    }
}
