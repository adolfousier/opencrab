//! Tests for the `web_scrape` tool surface and its no-network guard paths. The
//! scraping pipeline itself is covered by the per-stage module tests; here we
//! pin the tool contract (name, schema, approval-free Network capability) and
//! that bad input and SSRF-blocked URLs fail cleanly before any fetch.

use crate::brain::tools::r#trait::{Tool, ToolCapability, ToolExecutionContext};
use crate::brain::tools::web_scrape::WebScrapeTool;
use serde_json::json;
use uuid::Uuid;

fn ctx() -> ToolExecutionContext {
    ToolExecutionContext::new(Uuid::new_v4())
}

#[test]
fn advertises_stable_contract() {
    let tool = WebScrapeTool::default();
    assert_eq!(tool.name(), "web_scrape");
    // Network only — never gates on approval like a general file writer would.
    assert_eq!(tool.capabilities(), vec![ToolCapability::Network]);
    assert!(!tool.requires_approval());
}

#[test]
fn schema_requires_url_and_offers_modes() {
    let schema = WebScrapeTool::default().input_schema();
    assert_eq!(schema["required"][0], "url");
    let modes = &schema["properties"]["mode"]["enum"];
    assert!(modes.as_array().unwrap().iter().any(|m| m == "readable"));
    assert!(modes.as_array().unwrap().iter().any(|m| m == "sitemap"));
}

#[tokio::test]
async fn missing_url_is_invalid_input() {
    let err = WebScrapeTool::default()
        .execute(json!({}), &ctx())
        .await
        .expect_err("missing url must error");
    assert!(err.to_string().to_lowercase().contains("url"));
}

#[tokio::test]
async fn blank_url_is_invalid_input() {
    let err = WebScrapeTool::default()
        .execute(json!({ "url": "   " }), &ctx())
        .await
        .expect_err("blank url must error");
    assert!(err.to_string().to_lowercase().contains("url"));
}

#[tokio::test]
async fn ssrf_blocked_url_fails_without_fetch() {
    // file:// is rejected by the SSRF guard before any network call, so this
    // returns a clean error result rather than hanging or panicking.
    let result = WebScrapeTool::default()
        .execute(json!({ "url": "file:///etc/passwd" }), &ctx())
        .await
        .expect("execute should return a result, not an error");
    assert!(!result.success);
    assert!(result.error.unwrap_or(result.output).contains("web_scrape"));
}

#[tokio::test]
async fn localhost_url_is_blocked() {
    let result = WebScrapeTool::default()
        .execute(json!({ "url": "http://localhost:8080/admin" }), &ctx())
        .await
        .expect("execute should return a result");
    assert!(!result.success);
}
