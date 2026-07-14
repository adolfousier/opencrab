//! Tests for `HttpClientTool` — the `http_request` tool.
//!
//! These tests exist to pin the User-Agent default behaviour after
//! the `ffa1329` fix. Every http_request failure on the 2026-04-17
//! logs was GitHub returning `403 Forbidden` with "Request forbidden
//! by administrative rules. Please make sure your request has a
//! User-Agent header." reqwest ships with no default UA, and the
//! model had no way to know GitHub mandates one — the tool now sets
//! `opencrabs/<CARGO_PKG_VERSION>` automatically on every request.
//!
//! We use mockito to stand up a local HTTP server and assert the
//! UA header is present on the request reqwest actually sent.

use crate::brain::tools::http::HttpClientTool;
use crate::brain::tools::{Tool, ToolExecutionContext};
use serde_json::json;
use uuid::Uuid;

fn ctx() -> ToolExecutionContext {
    ToolExecutionContext::new(Uuid::new_v4()).with_auto_approve(true)
}

#[tokio::test]
async fn default_user_agent_is_opencrabs_with_version() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();
    let expected_ua = concat!("opencrabs/", env!("CARGO_PKG_VERSION"));

    let _mock = server
        .mock("GET", "/anything")
        .match_header("user-agent", expected_ua)
        .with_status(200)
        .with_body("ok")
        .create_async()
        .await;

    let tool = HttpClientTool;
    let input = json!({
        "method": "GET",
        "url": format!("{}/anything", url),
    });
    let result = tool.execute(input, &ctx()).await.expect("tool execute");
    assert!(
        result.success,
        "request should succeed when UA matches expected default, got: {:?}",
        result.output
    );
}

#[tokio::test]
async fn caller_supplied_user_agent_overrides_default() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    // mockito matches headers case-insensitively, so "User-Agent" vs
    // "user-agent" is not an issue here. We just need the VALUE to
    // be the caller's custom string, NOT the default opencrabs/X.Y.Z.
    let _mock = server
        .mock("GET", "/anything")
        .match_header("user-agent", "my-custom-agent/1.0")
        .with_status(200)
        .with_body("ok")
        .create_async()
        .await;

    let tool = HttpClientTool;
    let input = json!({
        "method": "GET",
        "url": format!("{}/anything", url),
        "headers": { "User-Agent": "my-custom-agent/1.0" },
    });
    let result = tool.execute(input, &ctx()).await.expect("tool execute");
    assert!(
        result.success,
        "caller-supplied UA should override default, got: {:?}",
        result.output
    );
}

#[tokio::test]
async fn forbidden_response_surfaces_body_to_caller() {
    // Regression: the original GitHub 403s included a helpful body
    // ("Please make sure your request has a User-Agent header"). The
    // tool needs to propagate that body to the LLM so when the fix
    // doesn't help (e.g. a future provider banning UA=opencrabs/*),
    // the model sees the actual reason instead of an opaque failure.
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    let _mock = server
        .mock("GET", "/forbidden")
        .with_status(403)
        .with_body("Request forbidden by administrative rules. Please make sure your request has a User-Agent header")
        .create_async()
        .await;

    let tool = HttpClientTool;
    let input = json!({
        "method": "GET",
        "url": format!("{}/forbidden", url),
    });
    let result = tool.execute(input, &ctx()).await.expect("tool execute");
    // The request COMPLETED (the server answered 403), so the tool succeeded;
    // a non-2xx status is data for the model, not a tool failure (#574). What
    // matters is that the status and the server's explanation reach the model.
    assert!(
        result.success,
        "a completed HTTP response is a successful tool invocation"
    );
    let body = result.output;
    assert!(
        body.contains("403"),
        "output should mention the status code: {}",
        body
    );
    assert!(
        body.contains("User-Agent header"),
        "output should surface the server's explanation: {}",
        body
    );
    assert!(
        body.contains("non-2xx"),
        "output should flag the non-2xx status so the model notices: {}",
        body
    );
    assert_eq!(
        result.metadata.get("status_code").map(String::as_str),
        Some("403"),
        "status code stays available in metadata"
    );
}

#[tokio::test]
async fn not_found_is_a_successful_invocation_with_status_surfaced() {
    // A 404 is a valid answer to "does this exist?" — the request completed, so
    // the tool succeeded. The status must still be visible to the model (#574).
    let mut server = mockito::Server::new_async().await;
    let url = server.url();
    let _mock = server
        .mock("GET", "/missing")
        .with_status(404)
        .with_body("not here")
        .create_async()
        .await;

    let tool = HttpClientTool;
    let input = json!({
        "method": "GET",
        "url": format!("{}/missing", url),
    });
    let result = tool.execute(input, &ctx()).await.expect("tool execute");
    assert!(
        result.success,
        "a completed 404 response is a successful tool invocation"
    );
    assert!(
        result.output.contains("404") && result.output.contains("non-2xx"),
        "status and non-2xx note must reach the model: {}",
        result.output
    );
    assert_eq!(
        result.metadata.get("status_code").map(String::as_str),
        Some("404")
    );
}

#[tokio::test]
async fn server_error_is_still_a_successful_invocation() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();
    let _mock = server
        .mock("GET", "/boom")
        .with_status(500)
        .with_body("kaboom")
        .create_async()
        .await;

    let tool = HttpClientTool;
    let input = json!({ "method": "GET", "url": format!("{}/boom", url) });
    let result = tool.execute(input, &ctx()).await.expect("tool execute");
    assert!(
        result.success,
        "a completed 5xx response is not a tool failure"
    );
    assert!(result.output.contains("kaboom"), "body reaches the model");
}
