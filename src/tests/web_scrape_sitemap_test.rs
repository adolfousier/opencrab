//! Tests for sitemap XML parsing. Prove `<loc>` extraction pulls every page URL,
//! decodes the `&amp;` entities generators emit in query strings, resolves
//! relative locs against the sitemap's own URL, and that the sitemap sniffer
//! recognizes both `<urlset>` leaves and `<sitemapindex>` documents. These are
//! the pure, network-free halves of the crawler; the async walk that fetches
//! nested sitemaps is exercised end-to-end, not here.

use crate::brain::tools::web_scrape::sitemap::{extract_locs, looks_like_sitemap};
use url::Url;

#[test]
fn extracts_all_page_locs() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
          <url><loc>https://example.com/a</loc></url>
          <url><loc>https://example.com/b</loc></url>
          <url><loc>https://example.com/c</loc></url>
        </urlset>"#;
    let locs = extract_locs(xml, None);
    assert_eq!(locs.len(), 3);
    assert!(locs.contains(&"https://example.com/a".to_string()));
    assert!(locs.contains(&"https://example.com/c".to_string()));
}

#[test]
fn decodes_amp_entities_in_query_strings() {
    // Sitemap generators escape `&` as `&amp;` inside URLs; the raw loc must be
    // decoded back to a real, fetchable URL.
    let xml = r#"<urlset><url><loc>https://example.com/p?a=1&amp;b=2</loc></url></urlset>"#;
    let locs = extract_locs(xml, None);
    assert_eq!(locs, vec!["https://example.com/p?a=1&b=2".to_string()]);
}

#[test]
fn resolves_relative_locs_against_base() {
    // Rare but legal: a loc expressed relative to the sitemap's own location.
    let base = Url::parse("https://example.com/sitemap.xml").unwrap();
    let xml = r#"<urlset><url><loc>/blog/post-1</loc></url></urlset>"#;
    let locs = extract_locs(xml, Some(&base));
    assert_eq!(locs, vec!["https://example.com/blog/post-1".to_string()]);
}

#[test]
fn drops_relative_locs_without_a_base() {
    // With no base to resolve against, an un-absolutizable loc is dropped rather
    // than handed back as a broken relative string.
    let xml = r#"<urlset><url><loc>/blog/post-1</loc></url></urlset>"#;
    let locs = extract_locs(xml, None);
    assert!(locs.is_empty());
}

#[test]
fn extracts_nested_sitemap_locs_from_index() {
    // A `<sitemapindex>` lists child sitemaps as `<loc>` too — same extraction.
    let xml = r#"<sitemapindex>
          <sitemap><loc>https://example.com/sitemap-posts.xml</loc></sitemap>
          <sitemap><loc>https://example.com/sitemap-pages.xml</loc></sitemap>
        </sitemapindex>"#;
    let locs = extract_locs(xml, None);
    assert_eq!(locs.len(), 2);
    assert!(locs.contains(&"https://example.com/sitemap-posts.xml".to_string()));
}

#[test]
fn recognizes_urlset_and_index_documents() {
    assert!(looks_like_sitemap(
        r#"<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">"#
    ));
    assert!(looks_like_sitemap("<sitemapindex>"));
    assert!(!looks_like_sitemap(
        "<html><body>not a sitemap</body></html>"
    ));
}
