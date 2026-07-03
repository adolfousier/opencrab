//! Tests for the genericized structural cleaner. The key guarantees: script/
//! style/handler/comment noise is removed, image and link URLs are preserved
//! (they must survive into the markdown so the agent can vision them on
//! demand), and none of insight_forge's Portuguese/client-specific line
//! filters leak in to delete legitimate content.

use crate::brain::tools::web_scrape::clean::{
    collapse_blank_lines, decode_html_entities, strip_noise, to_plain_text,
};

#[test]
fn strip_noise_removes_script_and_style_blocks() {
    let html =
        r#"<p>keep me</p><script>alert('x')</script><style>.a{color:red}</style><p>and me</p>"#;
    let out = strip_noise(html);
    assert!(out.contains("keep me"));
    assert!(out.contains("and me"));
    assert!(!out.contains("alert"));
    assert!(!out.contains("color:red"));
}

#[test]
fn strip_noise_removes_inline_handlers_and_comments() {
    let html = r#"<a href="/x" onclick="steal()">link</a><!-- tracking pixel -->"#;
    let out = strip_noise(html);
    assert!(!out.contains("onclick"));
    assert!(!out.contains("steal"));
    assert!(!out.contains("tracking pixel"));
    // The href and the visible text survive.
    assert!(out.contains(r#"href="/x""#));
    assert!(out.contains("link"));
}

#[test]
fn strip_noise_preserves_image_and_link_urls() {
    // Images and links are the payload the agent visions on demand — the
    // cleaner must never strip their URLs.
    let html = r#"<img src="https://cdn.example.com/chart.png" alt="Q3 chart"><a href="https://example.com/report">report</a>"#;
    let out = strip_noise(html);
    assert!(out.contains("https://cdn.example.com/chart.png"));
    assert!(out.contains("https://example.com/report"));
    assert!(out.contains("Q3 chart"));
}

#[test]
fn decode_html_entities_maps_common_ones() {
    assert_eq!(decode_html_entities("a&amp;b"), "a&b");
    assert_eq!(decode_html_entities("x&lt;y&gt;z"), "x<y>z");
    assert_eq!(decode_html_entities("&quot;q&quot;"), "\"q\"");
    // nbsp becomes a plain space.
    assert_eq!(decode_html_entities("a&nbsp;b"), "a b");
}

#[test]
fn collapse_blank_lines_reduces_runs() {
    assert_eq!(collapse_blank_lines("a\n\n\n\n\nb"), "a\n\nb");
    assert_eq!(collapse_blank_lines("\n\nhello\n\n"), "hello");
}

#[test]
fn to_plain_text_end_to_end() {
    let html = r#"
        <div>
          <h1>Title</h1>
          <script>var x = 1;</script>
          <p>First   paragraph with   spaces.</p>
          <p>Second&nbsp;paragraph.</p>
        </div>
    "#;
    let text = to_plain_text(html);
    assert!(text.contains("Title"));
    assert!(text.contains("First paragraph with spaces."));
    assert!(text.contains("Second paragraph."));
    assert!(!text.contains("var x"));
    // No raw tags remain.
    assert!(!text.contains('<'));
    assert!(!text.contains('>'));
}

#[test]
fn to_plain_text_keeps_generic_form_words() {
    // insight_forge's cleaner deleted lines containing "Contact", "Nome:",
    // "Privacy", etc. On a generic site those are legitimate content and must
    // survive.
    let html = "<p>Contact us for a quote.</p><p>Your Privacy matters to us.</p>";
    let text = to_plain_text(html);
    assert!(text.contains("Contact us for a quote."));
    assert!(text.contains("Your Privacy matters to us."));
}
