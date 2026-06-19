//! Sentinel tests for the web / GitHub / browser routing rule.
//!
//! Regression context (2026-05-30): on multiple user sessions the
//! agent reached for `browser_navigate` when asked to "check the
//! GitHub PR" or "look up the docs for X" — the right tools were
//! the `gh` CLI via bash and a search tool respectively. Root
//! cause: the tool descriptions said WHAT each tool did but never
//! WHEN to pick it, and the brain preamble had no routing rule.
//!
//! These tests pin three contracts that together force the routing:
//!
//! 1. `BRAIN_PREAMBLE` carries the WEB / GITHUB / BROWSER ROUTING
//!    block so the rule lands in the system prompt every turn
//!    (not just when the user has loaded `TOOLS.md`).
//! 2. `browser_navigate.description` calls out research / GitHub
//!    as the wrong use case — the model sees this in its tool list.
//! 3. The search tools + `bash` carry positive guidance ("prefer
//!    me for X") so the routing rule has somewhere to land.
//!
//! `browser_navigate.description` is checked via `include_str!` of
//! the source file rather than instantiating the tool — the real
//! constructor needs an `Arc<BrowserManager>`, which costs a real
//! Chrome handshake we don't want in a unit test.

use crate::brain::prompt_builder::BRAIN_PREAMBLE;
use crate::brain::tools::Tool;
use crate::brain::tools::bash::BashTool;
use crate::brain::tools::brave_search::BraveSearchTool;
use crate::brain::tools::exa_search::ExaSearchTool;
use crate::brain::tools::http::HttpClientTool;
use crate::brain::tools::web_search::WebSearchTool;

const BROWSER_NAVIGATE_SRC: &str = include_str!("../brain/tools/browser/navigate.rs");

#[test]
fn brain_preamble_carries_routing_block() {
    // The block must include all three surfaces (search / gh / browser)
    // and the "last resort" framing for browser. Without all three,
    // the model still has freedom to misroute.
    assert!(
        BRAIN_PREAMBLE.contains("WEB / GITHUB / BROWSER ROUTING"),
        "the routing section header must be present in BRAIN_PREAMBLE \
         so the rule reaches the model every turn"
    );
    assert!(
        BRAIN_PREAMBLE.contains("exa_search")
            && BRAIN_PREAMBLE.contains("brave_search")
            && BRAIN_PREAMBLE.contains("web_search"),
        "preamble must name all three search tools so the model knows \
         the preference order"
    );
    assert!(
        BRAIN_PREAMBLE.contains("`gh` CLI"),
        "preamble must name the gh CLI as the GitHub surface"
    );
    assert!(
        BRAIN_PREAMBLE.contains("last resort"),
        "browser must be framed as a last resort, not just an option"
    );
}

#[test]
fn browser_navigate_description_warns_against_research_misuse() {
    // Source-level check — BrowserNavigateTool::new() needs a real
    // BrowserManager so direct instantiation is overkill. The
    // description literal must carry the routing guardrails.
    let src = BROWSER_NAVIGATE_SRC;
    assert!(
        src.contains("DO NOT use for research"),
        "browser_navigate description must explicitly forbid the \
         research misuse pattern"
    );
    assert!(
        src.contains("GitHub"),
        "browser_navigate description must call out GitHub specifically \
         — that's the most common misroute"
    );
    assert!(
        src.contains("exa_search") || src.contains("brave_search") || src.contains("web_search"),
        "browser_navigate description must point at the search \
         alternatives so the model has a place to redirect"
    );
    assert!(
        src.contains("last resort"),
        "browser_navigate must be framed as a last resort, not a default"
    );
}

#[test]
fn web_search_description_positions_as_default() {
    let desc = WebSearchTool.description();
    assert!(
        desc.contains("DEFAULT") || desc.contains("default"),
        "web_search must announce itself as the default research tool: {desc}"
    );
    assert!(
        desc.contains("gh"),
        "web_search must point at the gh CLI for GitHub-specific lookups: {desc}"
    );
}

#[test]
fn exa_search_description_announces_preference_over_web_search() {
    let tool = ExaSearchTool::new(None);
    let desc = tool.description();
    assert!(
        desc.contains("PREFERRED over `web_search`"),
        "exa_search must announce preference over web_search so the \
         model picks it when both are available: {desc}"
    );
}

#[test]
fn brave_search_description_announces_preference_over_web_search() {
    let tool = BraveSearchTool::new("dummy-key".to_string());
    let desc = tool.description();
    assert!(
        desc.contains("PREFERRED over `web_search`"),
        "brave_search must announce preference over web_search: {desc}"
    );
    assert!(
        desc.contains("current events") || desc.contains("news"),
        "brave_search must surface its strength (current events / news) \
         so the model knows when to pick it over exa: {desc}"
    );
}

#[test]
fn brain_preamble_routes_specific_url_content_to_http_request() {
    // Regression (2026-06-19): the agent reached for `browser_navigate` when
    // the user "just said to check the website content". Research routing
    // covered open-ended queries, but reading a SPECIFIC URL the user handed
    // over had no documented home, so the model defaulted to the browser.
    assert!(
        BRAIN_PREAMBLE.contains("http_request"),
        "preamble must name http_request as the surface for fetching a \
         specific URL's content"
    );
    assert!(
        BRAIN_PREAMBLE.contains("check the website content"),
        "preamble must explicitly route the 'check the website content' \
         phrasing away from the browser"
    );
}

#[test]
fn http_request_description_advertises_page_content_fetch() {
    // The model only routes "read this URL" to http_request if the tool's
    // own description says it fetches page content — otherwise it reads as
    // an API-only tool and the model falls back to the browser.
    let desc = HttpClientTool.description();
    assert!(
        desc.contains("web page") || desc.contains("URL"),
        "http_request must advertise that it fetches a web page / URL's \
         content, not just 'external APIs': {desc}"
    );
    assert!(
        desc.contains("browser_navigate"),
        "http_request must point at browser_navigate as the escalation for \
         JS-rendered / interactive pages: {desc}"
    );
}

#[test]
fn browser_navigate_redirects_url_reads_to_http_request() {
    // The browser's own description must redirect "just read this URL" to
    // http_request so the model has a concrete cheaper alternative.
    assert!(
        BROWSER_NAVIGATE_SRC.contains("http_request"),
        "browser_navigate description must point at http_request for \
         reading a specific URL's content"
    );
}

#[test]
fn bash_description_routes_github_through_gh_cli() {
    let desc = BashTool.description();
    assert!(
        desc.contains("GITHUB OPERATIONS") || desc.contains("gh"),
        "bash must call out the gh CLI as the GitHub surface: {desc}"
    );
    assert!(
        desc.contains("browser_navigate"),
        "bash must explicitly tell the model not to reach for \
         browser_navigate for GitHub: {desc}"
    );
    assert!(
        desc.contains("--json") || desc.contains("structured JSON"),
        "bash must mention gh's structured-JSON output so the model \
         picks --json over scraping: {desc}"
    );
}
