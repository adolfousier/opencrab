//! #1221/#1225: the background-task echo bubble renderer + the
//! session_notify label plumbing. Pure assertions — no bot, no runtime:
//! framing strips (System + notify header), bubble assembly (rich markdown +
//! HTML fallback), truncation discipline (raw text cut BEFORE conversion,
//! wrapper tags stay intact).

use crate::channels::telegram::resume::{
    background_task_title, build_bg_echo_bubble, split_bg_echo_parts, split_notify_header,
    strip_system_framing,
};
use uuid::Uuid;

const SENDER: &str = "6c1c9cb9-8243-4def-abe5-d926d0ca8bed";

#[test]
fn strips_notify_header_and_returns_sender() {
    let ctx = format!("[session-notify from={SENDER}]\n\nhello from the other topic");
    let (sender, body) = split_bg_echo_parts(&ctx);
    assert_eq!(sender, Some(Uuid::parse_str(SENDER).unwrap()));
    assert_eq!(body, "hello from the other topic");
}

#[test]
fn split_notify_header_rejects_malformed_framing() {
    let bad_uuid = "[session-notify from=not-a-uuid]\n\nbody";
    let (sender, body) = split_notify_header(bad_uuid);
    assert_eq!(sender, None);
    assert_eq!(body, bad_uuid, "malformed header must pass whole text through");
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
    assert_eq!(inner, ctx, "the ']' must terminate the block to be stripped");
}

#[test]
fn strips_system_framing_even_after_notify_header() {
    let ctx = format!(
        "[session-notify from={SENDER}]\n\n[System: the push you asked for]\npushed body"
    );
    let (sender, body) = split_bg_echo_parts(&ctx);
    assert!(sender.is_some());
    assert!(!body.contains("[System:"), "System framing stripped after header");
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