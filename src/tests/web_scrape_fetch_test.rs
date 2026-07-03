//! Tests for the `web_scrape` fetch stage. Only the network-free `is_js_shell`
//! heuristic is exercised here: the reqwest and headless-Chrome paths are I/O
//! and are covered end-to-end elsewhere. The heuristic is what decides whether a
//! cheap static fetch is enough or the page needs a browser render, so its
//! boundaries are worth pinning down.

use crate::brain::tools::web_scrape::fetch::is_js_shell;

#[test]
fn server_rendered_page_is_not_a_shell() {
    let html = "<html><body><article><h1>Real Article</h1><p>\
        This is a fully server-rendered page with plenty of visible prose. \
        It has multiple sentences of genuine content that a reader can see \
        without running any JavaScript at all, so it must not be flagged as \
        an empty shell needing a browser render.</p></article></body></html>";
    assert!(!is_js_shell(html));
}

#[test]
fn empty_spa_shell_with_scripts_is_flagged() {
    let html = "<html><head><script src=\"/bundle.js\"></script></head>\
        <body><div id=\"root\"></div><script>window.__APP__=1;</script></body></html>";
    assert!(is_js_shell(html));
}

#[test]
fn empty_page_without_scripts_is_not_a_shell() {
    // No scripts at all means there is nothing a browser render would reveal,
    // so even a sparse page should not escalate.
    let html = "<html><body><div id=\"root\"></div></body></html>";
    assert!(!is_js_shell(html));
}

#[test]
fn script_heavy_page_with_real_text_is_not_a_shell() {
    // Scripts present, but the body already carries the content: no escalation.
    let html = "<html><head><script src=\"/analytics.js\"></script></head><body>\
        <main><p>The quarterly results are in and revenue grew across every \
        region we operate in. Below is a detailed breakdown of the numbers, \
        the drivers behind them, and what we expect for the next period. Growth \
        was strongest in the enterprise segment, where new logos and expansion \
        deals both outpaced our internal targets, while the self-serve tier held \
        steady quarter over quarter. Costs stayed flat, so most of that top-line \
        gain fell through to margin.</p></main><script>track();</script></body></html>";
    assert!(!is_js_shell(html));
}
