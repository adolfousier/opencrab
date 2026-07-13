//! Tests for `web_search`: the DuckDuckGo User-Agent rotation pool (#525) and
//! the pure result parser / formatter. The retry loop itself is network I/O and
//! is exercised in the field; these pin the rotation intent and the parsing.

use crate::brain::tools::web_search::{DDG_USER_AGENTS, format_results, parse_lite_results};

#[test]
fn ua_pool_has_multiple_distinct_agents() {
    // Rotation needs more than one UA; collapsing back to a single hardcoded UA
    // would reintroduce the 403 failures this fixes (#525).
    assert!(DDG_USER_AGENTS.len() >= 3, "need a pool to rotate through");
    let mut seen = std::collections::HashSet::new();
    for ua in DDG_USER_AGENTS {
        assert!(ua.starts_with("Mozilla/5.0"), "realistic UA expected: {ua}");
        assert!(seen.insert(*ua), "UAs must be distinct: {ua}");
    }
}

#[test]
fn parse_lite_extracts_result_links() {
    let html = r#"
        <a class="result-link" href="https://example.com/a">First result</a>
        <a class="result-link" href="https://rust-lang.org">Rust</a>
        <a class="result-link" href="ftp://skip.me">not http</a>
        <a class="result-link" href="https://empty.com"></a>
    "#;
    let results = parse_lite_results(html, 5);
    assert_eq!(
        results.len(),
        2,
        "non-http and empty-title links are dropped"
    );
    assert_eq!(results[0].title, "First result");
    assert_eq!(results[0].url, "https://example.com/a");
    assert_eq!(results[1].url, "https://rust-lang.org");
}

#[test]
fn parse_lite_respects_max_results() {
    let html = r#"
        <a class="result-link" href="https://a.com">A</a>
        <a class="result-link" href="https://b.com">B</a>
        <a class="result-link" href="https://c.com">C</a>
    "#;
    assert_eq!(parse_lite_results(html, 2).len(), 2);
}

#[test]
fn format_results_renders_list_and_empty() {
    let results = parse_lite_results(
        r#"<a class="result-link" href="https://example.com">Ex</a>"#,
        5,
    );
    let out = format_results("rust async", &results);
    assert!(out.contains("Search results for: \"rust async\""));
    assert!(out.contains("1. Ex"));
    assert!(out.contains("https://example.com"));

    let empty = format_results("nope", &[]);
    assert!(empty.contains("No results found"));
}
