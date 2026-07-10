//! Tests for Telegram handler: `split_message`, `markdown_to_telegram_html`, `escape_html`.

use crate::channels::telegram::handler::{
    build_midturn_queued_message, escape_html, markdown_to_telegram_html, split_message,
};

// ── split_message ─────────────────────────────────────────────────────

#[test]
fn split_short_message() {
    let chunks = split_message("hello", 4096);
    assert_eq!(chunks, vec!["hello"]);
}

#[test]
fn split_long_message() {
    let text = "a\n".repeat(3000);
    let chunks = split_message(&text, 4096);
    assert!(chunks.len() >= 2);
    for chunk in &chunks {
        assert!(chunk.len() <= 4096);
    }
    let joined: String = chunks.into_iter().collect();
    assert_eq!(joined, text);
}

#[test]
fn split_no_newlines() {
    let text = "a".repeat(5000);
    let chunks = split_message(&text, 4096);
    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0].len(), 4096);
    assert_eq!(chunks[1].len(), 904);
}

// ── markdown_to_telegram_html ─────────────────────────────────────────

#[test]
fn markdown_bold() {
    let html = markdown_to_telegram_html("**hello**");
    assert!(html.contains("<b>hello</b>"));
}

#[test]
fn markdown_code_block() {
    let md = "```rust\nfn main() {}\n```";
    let html = markdown_to_telegram_html(md);
    assert!(html.contains("<pre><code"));
    assert!(html.contains("fn main()"));
    assert!(html.contains("</code></pre>"));
}

#[test]
fn markdown_inline_code() {
    let html = markdown_to_telegram_html("use `cargo build`");
    assert!(html.contains("<code>cargo build</code>"));
}

// ── escape_html ───────────────────────────────────────────────────────

#[test]
fn escape_html_tags() {
    assert_eq!(
        escape_html("<script>alert('xss')</script>"),
        "&lt;script&gt;alert('xss')&lt;/script&gt;"
    );
}

#[test]
fn escape_html_ampersand() {
    assert_eq!(escape_html("a & b"), "a &amp; b");
}

// ── IMG marker format ─────────────────────────────────────────────────

#[test]
fn img_marker_format() {
    let path = "/tmp/tg_photo_abc.jpg";
    let caption = "What's in this image?";
    let text = format!("<<IMG:{}>> {}", path, caption);
    assert!(text.starts_with("<<IMG:"));
    assert!(text.contains(path));
    assert!(text.contains(caption));
}

// ── build_midturn_queued_message: slash command vs plain follow-up ───
// A slash command that lands mid-turn is a deliberate NEW directive and must
// NOT get the "fold into the current task, do not restart" wrapper a plain
// follow-up gets — that wrapper neutralized /drop_release so the release
// never ran. Slash commands get a directive wrapper and show the command in
// history; plain follow-ups keep the original behavior byte-for-byte.

#[test]
fn slash_command_midturn_is_a_distinct_directive() {
    let body = "# Drop Release\nYou are preparing a new release. Follow every step.";
    let q = build_midturn_queued_message(Some("/drop_release"), body, "ignored display");
    // Names the command and frames it as a NEW directive, not a refinement.
    assert!(q.context_text.contains("/drop_release"));
    assert!(q.context_text.contains("explicit NEW directive"));
    assert!(
        q.context_text
            .contains("carry out the following instructions")
    );
    // The resolved body is carried through so the command can actually run.
    assert!(q.context_text.contains("preparing a new release"));
    // It must NOT carry the follow-up "do not restart" framing.
    assert!(!q.context_text.contains("do not restart from scratch"));
    // History shows the command the user typed, not the whole body.
    assert_eq!(q.display_text, "/drop_release");
}

#[test]
fn plain_followup_midturn_keeps_the_fold_in_wrapper() {
    let q = build_midturn_queued_message(None, "resolved", "also check the logs");
    assert!(q.context_text.contains("factor it into the CURRENT task"));
    assert!(q.context_text.contains("do not restart from scratch"));
    assert!(q.context_text.contains("also check the logs"));
    // A plain follow-up must NOT read as a command invocation.
    assert!(!q.context_text.contains("explicit NEW directive"));
    assert_eq!(q.display_text, "also check the logs");
}
