//! Sitemap discovery and URL enumeration for `web_scrape`.
//!
//! Ported and simplified from insight_forge's sitemap crawler. Two jobs:
//! [`discover_sitemap_url`] locates a site's sitemap (well-known path, then
//! `robots.txt` `Sitemap:` directives, then common alternates), and
//! [`collect_sitemap_urls`] walks it into a flat list of page URLs, following
//! `<sitemapindex>` entries into their child sitemaps.
//!
//! The XML is parsed by pulling `<loc>` values with a regex rather than a full
//! XML parser: sitemaps are machine-generated and always express both page URLs
//! and nested-sitemap URLs as `<loc>`, so this reuses the existing `regex`
//! dependency and adds nothing new to the tree. Recursion is handled with an
//! explicit work queue (not async recursion), which bounds total fetches, dedups
//! visited sitemaps, and needs no boxed futures.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;
use url::Url;

use super::fetch::fetch_static;

/// Captures the text content of a sitemap `<loc>…</loc>` element.
static LOC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<loc>\s*([^<\s][^<]*?)\s*</loc>").unwrap());

/// Safety cap on how many sitemap documents a single crawl will fetch, so a
/// pathological sitemap index can't fan out into an unbounded fetch storm.
const MAX_SITEMAP_DOCS: usize = 50;

/// Auto-discover a sitemap URL for `root_url`.
///
/// Tries, in order: `/sitemap.xml`, `Sitemap:` directives in `/robots.txt`, then
/// a handful of common alternate names. Returns the first URL whose body looks
/// like a real sitemap (`<urlset` or `<sitemapindex`), or `None` when nothing
/// matches.
pub async fn discover_sitemap_url(root_url: &str, timeout_secs: u64) -> Option<String> {
    let parsed = Url::parse(root_url).ok()?;
    let base = format!("{}://{}", parsed.scheme(), parsed.host_str()?);

    // Well-known location first.
    let primary = format!("{base}/sitemap.xml");
    if looks_like_sitemap(
        &fetch_static(&primary, timeout_secs)
            .await
            .unwrap_or_default(),
    ) {
        return Some(primary);
    }

    // robots.txt may point at the real sitemap.
    if let Ok(robots) = fetch_static(&format!("{base}/robots.txt"), timeout_secs).await {
        for line in robots.lines() {
            let trimmed = line.trim();
            if trimmed.to_ascii_lowercase().starts_with("sitemap:") {
                let url = trimmed["sitemap:".len()..].trim();
                if !url.is_empty() {
                    return Some(url.to_string());
                }
            }
        }
    }

    // Common alternate names used by CMSs and sitemap plugins.
    const ALTERNATES: &[&str] = &[
        "/sitemap_index.xml",
        "/sitemap-index.xml",
        "/wp-sitemap.xml",
        "/sitemap/sitemap.xml",
    ];
    for alt in ALTERNATES {
        let candidate = format!("{base}{alt}");
        if looks_like_sitemap(
            &fetch_static(&candidate, timeout_secs)
                .await
                .unwrap_or_default(),
        ) {
            return Some(candidate);
        }
    }

    None
}

/// Walk `sitemap_url` into a flat, deduplicated list of page URLs.
///
/// A `<sitemapindex>` document contributes its `<loc>` entries as further
/// sitemaps to fetch; a plain `<urlset>` contributes its `<loc>` entries as page
/// URLs. Nested sitemaps are followed via an explicit work queue up to
/// [`MAX_SITEMAP_DOCS`] documents. Fetch failures on individual sitemaps are
/// logged and skipped rather than failing the whole crawl.
pub async fn collect_sitemap_urls(sitemap_url: &str, timeout_secs: u64) -> Vec<String> {
    let mut pages: Vec<String> = Vec::new();
    let mut seen_pages: HashSet<String> = HashSet::new();
    let mut visited: HashSet<String> = HashSet::new();
    let mut queue: Vec<String> = vec![sitemap_url.to_string()];
    let mut fetched = 0usize;

    while let Some(current) = queue.pop() {
        if !visited.insert(current.clone()) {
            continue;
        }
        if fetched >= MAX_SITEMAP_DOCS {
            tracing::debug!("web_scrape: sitemap crawl hit {MAX_SITEMAP_DOCS}-doc cap, stopping");
            break;
        }
        fetched += 1;

        let body = match fetch_static(&current, timeout_secs).await {
            Ok(b) => b,
            Err(e) => {
                tracing::debug!("web_scrape: sitemap {current} fetch failed: {e}");
                continue;
            }
        };

        let base = Url::parse(&current).ok();
        let locs = extract_locs(&body, base.as_ref());
        if body.contains("<sitemapindex") {
            // Index document: every loc is another sitemap to walk.
            queue.extend(locs);
        } else {
            // Leaf urlset: every loc is a page. Dedup as we go.
            for page in locs {
                if seen_pages.insert(page.clone()) {
                    pages.push(page);
                }
            }
        }
    }

    pages
}

/// Does this body look like a sitemap document?
pub fn looks_like_sitemap(body: &str) -> bool {
    body.contains("<urlset") || body.contains("<sitemapindex")
}

/// Pull every `<loc>` value out of a sitemap document.
///
/// Values are XML-entity decoded (sitemaps escape `&` as `&amp;`) and resolved
/// to absolute URLs against `base` when they arrive relative. Anything that
/// stays un-absolutizable is dropped.
pub fn extract_locs(xml: &str, base: Option<&Url>) -> Vec<String> {
    LOC.captures_iter(xml)
        .filter_map(|c| c.get(1))
        .map(|m| decode_xml_entities(m.as_str().trim()))
        .filter_map(|loc| {
            if loc.starts_with("http://") || loc.starts_with("https://") {
                Some(loc)
            } else {
                base.and_then(|b| b.join(&loc).ok()).map(|u| u.to_string())
            }
        })
        .collect()
}

/// Decode the five predefined XML entities. Sitemap `<loc>` values escape `&` in
/// query strings as `&amp;`; the others are decoded for completeness.
fn decode_xml_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}
