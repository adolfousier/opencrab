//! #1221: the background-task echo bubble renderer. Pure-HTML assertions so
//! no bot or runtime is needed: framing strip, rich-format preservation,
//! truncation discipline (raw text cut BEFORE conversion, tags stay intact).

use crate::channels::telegram::resume::render_bg_echo_html;

#[test]
fn wraps_output_in_expandable_blockquote_with_bold_header() {
    let html = render_bg_echo_html("[System: task finished]\nsome output");
    assert!(html.starts_with("<blockquote expandable>"));
    assert!(html.ends_with("</blockquote>"));
    assert!(html.contains("<b>⚙️ background task result</b>"));
    assert!(html.contains("some output"));
}

#[test]
fn strips_system_framing_from_display() {
    let html = render_bg_echo_html(
        "[System: the background task you started has finished.\nStatus: exit 0]\nreal tail",
    );
    assert!(!html.contains("[System:"), "scaffolding must not render");
    assert!(html.contains("Status: exit 0"), "inner content survives");
    assert!(html.contains("real tail"));
}

#[test]
fn preserves_markdown_as_rich_html() {
    let ctx = "[System: done]\n# Heading\n```rust\nfn main() {}\n```";
    let html = render_bg_echo_html(ctx);
    // Fences become block-level code, headings survive as markup — the echo
    // must NOT degrade to escaped plain text (#1221 requirement).
    assert!(
        html.contains("<pre") || html.contains("<code"),
        "fence lost: {html}"
    );
}

#[test]
fn long_output_is_truncated_before_conversion_and_stays_wellformed() {
    let ctx = format!("[System: done]\n{{}}<b>x</b>",);
    let big = format!("{ctx}\n{}", "y".repeat(10_000));
    let html = render_bg_echo_html(&big);
    assert!(html.contains("(truncated)"));
    // Truncating raw text first means the wrapper tags can never be cut:
    assert!(html.starts_with("<blockquote expandable>"));
    assert!(html.ends_with("</blockquote>"));
    // The literal HTML-ish junk inside the body got escaped, not interpreted.
    assert!(html.contains("&lt;b&gt;") || !html.contains("<b>x</b>"));
}

#[test]
fn non_system_shapes_pass_through_untouched() {
    let html = render_bg_echo_html("plain text, no framing");
    assert!(html.contains("plain text, no framing"));
}
