//! Structural noise removal for `web_scrape`, genericized from insight_forge's
//! `html_content_cleaner`.
//!
//! insight_forge's cleaner was tuned for one Portuguese SMB site: it dropped
//! lines containing `FORMULÁRIO`, `Nome:`, `Telefone:`, `Privacidade`, and so
//! on. Those string matches would delete legitimate content on an arbitrary
//! site, so none of them survive here. What remains is language-agnostic and
//! structural: remove `<script>`/`<style>` blocks and inline event handlers
//! (their source otherwise leaks as body text), strip HTML comments, decode the
//! common entities, and tidy whitespace.
//!
//! Image and link URLs are never touched. The whole point of the tool is that
//! images survive as markdown references the agent can vision on demand, so
//! there is deliberately no URL-stripping phase here.

use std::sync::LazyLock;

use regex::Regex;
use unicode_normalization::UnicodeNormalization;

static SCRIPT_BLOCK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<script[^>]*>.*?</script>").unwrap());
static STYLE_BLOCK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<style[^>]*>.*?</style>").unwrap());
static INLINE_HANDLER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)\s+on\w+\s*=\s*("[^"]*"|'[^']*')"#).unwrap());
static HTML_COMMENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<!--.*?-->").unwrap());
static HTML_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").unwrap());
static BLANK_LINES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n{3,}").unwrap());
static INLINE_WS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ \t]{2,}").unwrap());
static HTML_ENTITY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"&[a-zA-Z]+;|&#[0-9]+;|&#x[0-9a-fA-F]+;").unwrap());

/// Remove script/style blocks, inline event handlers, and HTML comments from
/// `html`. Structural only: real element text is untouched, and no `src`/`href`
/// is rewritten or removed.
pub fn strip_noise(html: &str) -> String {
    let mut out = SCRIPT_BLOCK.replace_all(html, "").to_string();
    out = STYLE_BLOCK.replace_all(&out, "").to_string();
    out = INLINE_HANDLER.replace_all(&out, "").to_string();
    out = HTML_COMMENT.replace_all(&out, "").to_string();
    out
}

/// Collapse three-or-more consecutive newlines to a single blank-line separator
/// and trim the ends. Run on converted markdown to tidy the spacing left around
/// removed blocks.
pub fn collapse_blank_lines(text: &str) -> String {
    BLANK_LINES.replace_all(text.trim(), "\n\n").to_string()
}

/// Decode the handful of HTML entities that actually show up in body text.
/// Unknown entities collapse to a space rather than being left as raw `&…;`
/// noise.
pub fn decode_html_entities(text: &str) -> String {
    HTML_ENTITY
        .replace_all(text, |caps: &regex::Captures| {
            match caps.get(0).unwrap().as_str() {
                "&nbsp;" => " ",
                "&amp;" => "&",
                "&lt;" => "<",
                "&gt;" => ">",
                "&quot;" => "\"",
                "&apos;" => "'",
                _ => " ",
            }
            .to_string()
        })
        .to_string()
}

/// Reduce `html` to plain text: strip noise, drop every remaining tag, decode
/// entities, NFC-normalize, collapse inline whitespace, and drop blank lines.
/// Used for the `text` output format; the markdown path uses the htmd converter
/// instead.
pub fn to_plain_text(html: &str) -> String {
    let stripped = strip_noise(html);
    let no_tags = HTML_TAG.replace_all(&stripped, " ").to_string();
    let decoded = decode_html_entities(&no_tags);
    let normalized: String = decoded.nfc().collect();
    let lines: Vec<String> = normalized
        .lines()
        .map(|line| INLINE_WS.replace_all(line.trim(), " ").trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();
    lines.join("\n")
}
