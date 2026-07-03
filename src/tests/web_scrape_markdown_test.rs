//! Tests for HTML-to-markdown conversion and URL absolutization. The headline
//! guarantee: images survive as markdown `![alt](absolute-url)` references so
//! the agent can vision the specific ones it needs, and relative URLs are
//! resolved against the page base so those references are directly fetchable.

use crate::brain::tools::web_scrape::to_markdown::{absolutize_urls, to_markdown};
use url::Url;

fn base() -> Url {
    Url::parse("https://example.com/blog/post").unwrap()
}

#[test]
fn absolutize_resolves_relative_src_and_href() {
    let html = r#"<img src="/img/a.png"><a href="../about">about</a>"#;
    let out = absolutize_urls(html, &base());
    assert!(out.contains(r#"src="https://example.com/img/a.png""#));
    assert!(out.contains(r#"href="https://example.com/about""#));
}

#[test]
fn absolutize_leaves_absolute_and_special_urls() {
    let html = r##"<img src="https://cdn.example.com/x.png"><a href="#section">jump</a><a href="mailto:a@b.com">mail</a>"##;
    let out = absolutize_urls(html, &base());
    assert!(out.contains(r#"src="https://cdn.example.com/x.png""#));
    assert!(out.contains(r##"href="#section""##));
    assert!(out.contains(r#"href="mailto:a@b.com""#));
}

#[test]
fn absolutize_handles_protocol_relative() {
    let html = r#"<img src="//cdn.example.com/y.png">"#;
    let out = absolutize_urls(html, &base());
    // Protocol-relative URLs pick up the page's https scheme.
    assert!(out.contains(r#"src="https://cdn.example.com/y.png""#));
}

#[test]
fn markdown_keeps_headings_and_links() {
    let md = to_markdown(r#"<h1>Title</h1><p>Some <a href="https://example.com/x">link</a> text.</p>"#);
    assert!(md.contains("# Title"));
    assert!(md.contains("[link](https://example.com/x)"));
    assert!(md.contains("text."));
}

#[test]
fn markdown_keeps_images_as_url_tags() {
    // The core design guarantee: an <img> becomes a markdown image reference,
    // never OCR'd or dropped.
    let md = to_markdown(r#"<p>chart:</p><img src="https://cdn.example.com/q3.png" alt="Q3 revenue">"#);
    assert!(md.contains("https://cdn.example.com/q3.png"));
    assert!(md.contains("Q3 revenue"));
    assert!(md.contains("!["));
}

#[test]
fn absolutize_then_markdown_yields_fetchable_image() {
    // End-to-end: a relative image on the page becomes an absolute markdown
    // image reference the agent can hand straight to vision.
    let html = r#"<main><p>See below. lorem ipsum dolor sit amet.</p><img src="/media/fig1.png" alt="Figure 1"></main>"#;
    let md = to_markdown(&absolutize_urls(html, &base()));
    assert!(md.contains("https://example.com/media/fig1.png"));
    assert!(md.contains("Figure 1"));
}
