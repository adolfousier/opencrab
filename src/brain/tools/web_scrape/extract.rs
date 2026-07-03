//! Main-content isolation for `web_scrape`, ported from insight_forge's
//! `extract_main_content`.
//!
//! Everything here is synchronous and returns owned `String`s on purpose:
//! `scraper`'s `Html`/`Selector` types hold `Rc` and are `!Send`, so keeping
//! them out of any `async fn` is what lets the tool's async `execute()` stay
//! `Send` (extraction runs to completion before the pipeline ever `.await`s).

use scraper::{Html, Selector};

/// Content-container selectors, tried in priority order. The first match whose
/// serialized HTML is substantial (> 100 chars) wins.
const MAIN_SELECTORS: &[&str] = &[
    "article#main-content",
    "main",
    ".main-content",
    "#content",
    ".content",
    "article",
    "[role='main']",
    "#main",
    ".article-content",
];

/// Structural elements removed from the `<body>` fallback — never real content.
const JUNK_SELECTORS: &[&str] = &[
    "header",
    "footer",
    "nav",
    "aside",
    ".sidebar",
    "#sidebar",
    ".widget",
    "form",
    "iframe",
    "noscript",
    ".advertisement",
    ".ads",
    ".cookie-notice",
    ".share-buttons",
    "[class*='social']",
];

/// Minimum serialized length for a container to count as "the main content".
const MIN_CONTENT_LEN: usize = 100;

/// Isolate the main content of `html` and return it as an HTML fragment.
///
/// Tries the priority container selectors first; if none has substantial
/// content, falls back to `<body>` with the structural junk elements stripped.
/// Returns the input unchanged only when there is no usable body, so the caller
/// always has something to convert.
pub fn extract_main_content(html: &str) -> String {
    let document = Html::parse_document(html);

    for selector in MAIN_SELECTORS {
        if let Ok(sel) = Selector::parse(selector)
            && let Some(element) = document.select(&sel).next()
        {
            let content = element.html();
            if content.trim().len() > MIN_CONTENT_LEN {
                return content;
            }
        }
    }

    // Body fallback: serialize <body>, then delete each junk element's markup.
    // Same approach as insight_forge — selector-driven string removal against
    // the serialized fragment.
    if let Ok(body_sel) = Selector::parse("body")
        && let Some(body) = document.select(&body_sel).next()
    {
        let mut content = body.html();
        for selector in JUNK_SELECTORS {
            if let Ok(sel) = Selector::parse(selector) {
                for element in document.select(&sel) {
                    let fragment = element.html();
                    if !fragment.is_empty() {
                        content = content.replace(&fragment, "");
                    }
                }
            }
        }
        if content.trim().len() > MIN_CONTENT_LEN {
            return content;
        }
    }

    html.to_string()
}
