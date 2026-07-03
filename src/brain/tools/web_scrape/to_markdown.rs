//! HTML-to-markdown conversion for `web_scrape`.
//!
//! Two steps. First resolve relative `src`/`href` attributes to absolute URLs
//! against the page's base, so image and link targets in the markdown are
//! directly fetchable and vision-able. Then convert to markdown with `htmd`.
//! Images come out as `![alt](absolute-url)`: the agent reads the page as text
//! and visions only the specific images a task needs — no AI runs here.

use std::sync::LazyLock;

use regex::Regex;
use url::Url;

/// Matches a `src="…"` or `href="…"` attribute (double-quoted). Capture 1 is
/// the leading whitespace + attribute name, capture 2 is the value.
static URL_ATTR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)(\s(?:src|href))\s*=\s*"([^"]*)""#).unwrap());

/// Rewrite relative `src`/`href` values in `html` to absolute URLs resolved
/// against `base`. Already-absolute URLs, in-page anchors (`#…`), and
/// `data:`/`mailto:`/`tel:`/`javascript:` URIs are left untouched.
pub fn absolutize_urls(html: &str, base: &Url) -> String {
    URL_ATTR
        .replace_all(html, |caps: &regex::Captures| {
            let attr = &caps[1];
            let resolved = resolve(&caps[2], base);
            format!(r#"{attr}="{resolved}""#)
        })
        .to_string()
}

/// Resolve a single attribute value against `base`, returning it unchanged when
/// it should not be rewritten or when resolution fails.
fn resolve(value: &str, base: &Url) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.starts_with('#')
        || trimmed.starts_with("data:")
        || trimmed.starts_with("mailto:")
        || trimmed.starts_with("tel:")
        || trimmed.starts_with("javascript:")
    {
        return value.to_string();
    }
    // Absolute already (has its own scheme+host) — keep as-is. Protocol-relative
    // (`//host/…`) fails this parse and correctly falls through to base.join,
    // which supplies the page's scheme.
    if Url::parse(trimmed).is_ok() {
        return value.to_string();
    }
    match base.join(trimmed) {
        Ok(absolute) => absolute.to_string(),
        Err(_) => value.to_string(),
    }
}

/// Convert an HTML fragment to markdown. Relative URLs should already be
/// absolutized via [`absolutize_urls`]. On the rare conversion error the input
/// is returned rather than panicking the tool.
pub fn to_markdown(html: &str) -> String {
    htmd::convert(html).unwrap_or_else(|_| html.to_string())
}
