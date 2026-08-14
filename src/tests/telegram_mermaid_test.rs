//! Tests for the Telegram mermaid-diagram render path (#1044).
//!
//! Pure, network-free coverage of the building blocks in
//! [`crate::channels::telegram::rich::mermaid`]: base64url encoding, fence
//! detection, the HTTP accept/reject decision, the failure-note builder, the
//! image/failure HTML shapes, and the block-resolution walk (exercised only
//! on non-mermaid blocks so no HTTP is performed). The config-gated entry
//! point `should_render_mermaid` is not unit-tested because it reads the live
//! `Config`, whose values in tests depend on the embedded example config.

use crate::channels::telegram::rich::api::build_body_markdown_media;
use crate::channels::telegram::rich::ast::{Block, Inline, MermaidResult};
use crate::channels::telegram::rich::markdown_to_html_mermaid;
use crate::channels::telegram::rich::mermaid::{
    MediaEntry, base64url, error_note, failure_html, find_mermaid_fences, has_mermaid_fence,
    image_html, is_image_response, markdown_failure_block, replacement_for, resolve_blocks,
    resolve_markdown_media,
};

// ---------------------------------------------------------------------------
// base64url
// ---------------------------------------------------------------------------

#[test]
fn base64url_matches_rfc4648_url_safe_no_pad() {
    // No special chars, padding stripped.
    assert_eq!(base64url("hello world"), "aGVsbG8gd29ybGQ");
    // Standard '+' maps to '-'.
    assert_eq!(base64url("~~~"), "fn5-");
    // Standard '/' maps to '_' (and padding still stripped).
    assert_eq!(base64url("????"), "Pz8_Pw");
}

#[test]
fn base64url_output_never_contains_forbidden_chars() {
    for input in [
        "a",
        "ab",
        "abc",
        "mermaid graph TD; A-->B",
        "héllo wörld",
        "????~~~~",
    ] {
        let out = base64url(input);
        assert!(!out.contains('+'), "unexpected '+' in {out}");
        assert!(!out.contains('/'), "unexpected '/' in {out}");
        assert!(!out.contains('='), "unexpected '=' padding in {out}");
    }
}

#[test]
fn base64url_round_trips() {
    use base64::Engine as _;
    for input in ["graph TD; A-->B;", "flowchart LR\n  X --> Y", "ünïcode ✓"] {
        let encoded = base64url(input);
        let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(encoded.as_bytes())
            .expect("base64url must decode");
        assert_eq!(String::from_utf8(decoded).unwrap(), input);
    }
}

// ---------------------------------------------------------------------------
// has_mermaid_fence
// ---------------------------------------------------------------------------

#[test]
fn has_mermaid_fence_detects_tagged_fence() {
    assert!(has_mermaid_fence("```mermaid\ngraph TD;\n```"));
    assert!(has_mermaid_fence("before\n```mermaid\nA-->B\n```\nafter"));
}

#[test]
fn has_mermaid_fence_is_case_insensitive_and_tolerates_space() {
    assert!(has_mermaid_fence("```Mermaid\ngraph TD;\n```"));
    assert!(has_mermaid_fence("``` mermaid\ngraph TD;\n```"));
}

#[test]
fn has_mermaid_fence_rejects_other_or_missing_fences() {
    assert!(!has_mermaid_fence("```rust\nfn main() {}\n```"));
    assert!(!has_mermaid_fence("plain prose, no fences"));
    assert!(!has_mermaid_fence("I like mermaid diagrams"));
    assert!(!has_mermaid_fence("```\nuntagged fence\n```"));
}

// ---------------------------------------------------------------------------
// is_image_response
// ---------------------------------------------------------------------------

#[test]
fn is_image_response_accepts_2xx_image() {
    assert!(is_image_response(200, "image/jpeg"));
    assert!(is_image_response(200, "image/png"));
    assert!(is_image_response(200, "image/svg+xml"));
    assert!(is_image_response(200, "image/png; charset=binary"));
    assert!(is_image_response(204, "image/webp"));
}

#[test]
fn is_image_response_is_case_insensitive_on_content_type() {
    assert!(is_image_response(200, "IMAGE/PNG"));
    assert!(is_image_response(200, "Image/Jpeg"));
}

#[test]
fn is_image_response_rejects_non_2xx_or_non_image() {
    assert!(!is_image_response(400, "image/jpeg"));
    assert!(!is_image_response(500, "image/png"));
    assert!(!is_image_response(200, "text/plain"));
    assert!(!is_image_response(200, "text/html"));
    assert!(!is_image_response(200, ""));
    // 300 is outside the 2xx success range.
    assert!(!is_image_response(300, "image/png"));
}

// ---------------------------------------------------------------------------
// error_note
// ---------------------------------------------------------------------------

#[test]
fn error_note_returns_body_when_present() {
    assert_eq!(
        error_note(400, "Parse error on line 2: got 'LINK'"),
        "Parse error on line 2: got 'LINK'"
    );
}

#[test]
fn error_note_trims_whitespace() {
    assert_eq!(error_note(400, "   some error   "), "some error");
}

#[test]
fn error_note_falls_back_to_status_on_empty_body() {
    assert_eq!(error_note(500, ""), "diagram renderer returned HTTP 500");
    assert_eq!(error_note(400, "   "), "diagram renderer returned HTTP 400");
}

#[test]
fn error_note_caps_length() {
    let long = "x".repeat(1000);
    let note = error_note(400, &long);
    assert_eq!(note.chars().count(), 400);
}

// ---------------------------------------------------------------------------
// image_html / failure_html
// ---------------------------------------------------------------------------

#[test]
fn image_html_wraps_url_in_figure() {
    assert_eq!(
        image_html("https://mermaid.ink/img/abc123"),
        "<figure><img src=\"https://mermaid.ink/img/abc123\"/></figure>"
    );
}

#[test]
fn image_html_escapes_url_entities() {
    assert_eq!(
        image_html("a&b<c>"),
        "<figure><img src=\"a&amp;b&lt;c&gt;\"/></figure>"
    );
}

#[test]
fn failure_html_contains_warning_error_and_source() {
    let html = failure_html("Parse error on line 2", "graph TD; A-->B");
    assert!(html.contains("<b>⚠️ Mermaid diagram could not be rendered</b>"));
    assert!(html.contains("<blockquote>Parse error on line 2</blockquote>"));
    assert!(html.contains("<pre><code>graph TD; A--&gt;B</code></pre>"));
}

#[test]
fn failure_html_escapes_error_and_source() {
    let html = failure_html("<script>alert(1)</script>", "a < b & c");
    assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(html.contains("a &lt; b &amp; c"));
    assert!(!html.contains("<script>"));
}

// ---------------------------------------------------------------------------
// resolve_blocks (non-mermaid only — no network)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn resolve_blocks_passes_through_non_mermaid() {
    let blocks = vec![
        Block::Paragraph(vec![Inline::Text("hello".into())]),
        Block::Code {
            lang: Some("rust".into()),
            text: "fn main() {}".into(),
        },
    ];
    let resolved = resolve_blocks(blocks.clone()).await;
    assert_eq!(resolved, blocks, "non-mermaid blocks must be untouched");
}

#[tokio::test]
async fn resolve_blocks_empty_input() {
    let resolved = resolve_blocks(Vec::new()).await;
    assert!(resolved.is_empty());
}

// ---------------------------------------------------------------------------
// markdown_to_html_mermaid (no mermaid fence — exercises the full
// parse -> resolve -> render pipeline without any network call)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn markdown_to_html_mermaid_renders_plain_markdown() {
    let html = markdown_to_html_mermaid("# Hi\n\nSome **bold** text.").await;
    assert_eq!(html, "<b>Hi</b>\n\nSome <b>bold</b> text.");
}

// ---------------------------------------------------------------------------
// find_mermaid_fences
// ---------------------------------------------------------------------------

#[test]
fn find_mermaid_fences_locates_single_fence_with_range_and_source() {
    let text = "before\n```mermaid\ngraph TD;\nA-->B\n```\nafter";
    let fences = find_mermaid_fences(text);
    assert_eq!(fences.len(), 1);
    let f = &fences[0];
    // start points at the opening fence line, end just past the closing one.
    assert_eq!(&text[f.start..f.end], "```mermaid\ngraph TD;\nA-->B\n```\n");
    assert_eq!(f.source, "graph TD;\nA-->B\n");
}

#[test]
fn find_mermaid_fences_locates_multiple_and_orders_them() {
    let text = "```mermaid\nA\n```\nmid\n```mermaid\nB\n```";
    let fences = find_mermaid_fences(text);
    assert_eq!(fences.len(), 2);
    assert_eq!(fences[0].source, "A\n");
    assert_eq!(fences[1].source, "B\n");
    assert!(fences[0].start < fences[1].start);
}

#[test]
fn find_mermaid_fences_ignores_non_mermaid_and_unclosed() {
    assert!(find_mermaid_fences("```rust\nfn main() {}\n```").is_empty());
    assert!(find_mermaid_fences("no fences").is_empty());
    // Unclosed fence: no closing ``` so nothing is captured.
    assert!(find_mermaid_fences("```mermaid\ngraph TD;\n").is_empty());
}

// ---------------------------------------------------------------------------
// replacement_for (pure, no network)
// ---------------------------------------------------------------------------

#[test]
fn replacement_for_image_emits_media_reference_and_entry() {
    let outcome = MermaidResult::Image("https://mermaid.ink/img/xyz".into());
    let (md, entry) = replacement_for(&outcome, 0, "graph TD;");
    assert_eq!(md, "![diagram](tg://photo?id=diag0)");
    let e = entry.expect("image outcome must carry a media entry");
    assert_eq!(e.id, "diag0");
    assert_eq!(e.url, "https://mermaid.ink/img/xyz");
}

#[test]
fn replacement_for_image_uses_fence_index_in_id() {
    let outcome = MermaidResult::Image("u".into());
    let (md, entry) = replacement_for(&outcome, 3, "src");
    assert_eq!(md, "![diagram](tg://photo?id=diag3)");
    assert_eq!(entry.unwrap().id, "diag3");
}

#[test]
fn replacement_for_failed_emits_failure_block_and_no_entry() {
    let outcome = MermaidResult::Failed("Parse error".into());
    let (md, entry) = replacement_for(&outcome, 0, "graph TD;");
    assert!(
        entry.is_none(),
        "failed outcome must not carry a media entry"
    );
    assert!(md.contains("Mermaid diagram could not be rendered"));
    assert!(md.contains("Parse error"));
    assert!(md.contains("graph TD;"));
}

// ---------------------------------------------------------------------------
// markdown_failure_block
// ---------------------------------------------------------------------------

#[test]
fn markdown_failure_block_contains_warning_error_and_source() {
    let md = markdown_failure_block("Parse error on line 2", "graph TD; A-->B");
    assert!(md.contains("> ⚠️ **Mermaid diagram could not be rendered**"));
    assert!(md.contains("Parse error on line 2"));
    assert!(md.contains("graph TD; A-->B"));
    assert!(md.contains("Source:"));
}

// ---------------------------------------------------------------------------
// build_body_markdown_media (JSON shape, matches validated prototype 1073)
// ---------------------------------------------------------------------------

#[test]
fn build_body_markdown_media_matches_prototype_shape() {
    let media = vec![MediaEntry {
        id: "diag0".into(),
        url: "https://mermaid.ink/img/abc".into(),
    }];
    let body = build_body_markdown_media(-100, None, "text", &media);
    assert_eq!(body["chat_id"], -100);
    assert_eq!(body["rich_message"]["markdown"], "text");
    let arr = body["rich_message"]["media"]
        .as_array()
        .expect("media array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["id"], "diag0");
    assert_eq!(arr[0]["media"]["type"], "photo");
    assert_eq!(arr[0]["media"]["media"], "https://mermaid.ink/img/abc");
    assert!(body.get("message_thread_id").is_none());
}

#[test]
fn build_body_markdown_media_includes_thread_id_when_present() {
    use teloxide::types::{MessageId, ThreadId};
    let body = build_body_markdown_media(-100, Some(ThreadId(MessageId(249))), "m", &[]);
    assert_eq!(body["message_thread_id"], 249);
}

// ---------------------------------------------------------------------------
// resolve_markdown_media (no mermaid fence — no network)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn resolve_markdown_media_passes_through_without_fences() {
    let text = "# Title\n\nSome **bold** text and a table:\n\n| a | b |\n|---|---|\n| 1 | 2 |";
    let (resolved, media) = resolve_markdown_media(text).await;
    assert_eq!(resolved, text, "no-fence text must be byte-identical");
    assert!(media.is_empty());
}
