//! Regression: a configured `[browser] cdp_endpoint` must actually connect.
//!
//! chromey's `Browser::connect` only auto-resolves the real
//! `/devtools/browser/<id>` websocket URL via `/json/version` when the scheme
//! is `http`/`https`. A bare `ws://host:port` (the form originally documented
//! for #189) is handed straight to the websocket client and Chromium rejects
//! the upgrade on the root path — so it silently fails to connect.
//!
//! `normalize_cdp_endpoint` rewrites a path-less ws/wss endpoint to http/https
//! so both documented forms work, while leaving a full devtools URL untouched.

#![cfg(feature = "browser")]

use crate::brain::tools::browser::manager::normalize_cdp_endpoint;

#[test]
fn bare_ws_endpoint_is_rewritten_to_http_for_resolution() {
    assert_eq!(
        normalize_cdp_endpoint("ws://localhost:9222"),
        "http://localhost:9222"
    );
    assert_eq!(
        normalize_cdp_endpoint("wss://example.com:9222"),
        "https://example.com:9222"
    );
}

#[test]
fn trailing_slash_and_whitespace_are_handled() {
    assert_eq!(
        normalize_cdp_endpoint("ws://localhost:9222/"),
        "http://localhost:9222"
    );
    assert_eq!(
        normalize_cdp_endpoint("  ws://localhost:9222  "),
        "http://localhost:9222"
    );
}

#[test]
fn http_endpoints_pass_through_unchanged() {
    assert_eq!(
        normalize_cdp_endpoint("http://localhost:9222"),
        "http://localhost:9222"
    );
    assert_eq!(
        normalize_cdp_endpoint("https://example.com:9222"),
        "https://example.com:9222"
    );
}

#[test]
fn full_devtools_ws_url_is_left_as_is() {
    // A real devtools websocket URL already has the /devtools/browser/<id>
    // path and must be passed straight through — rewriting it would break it.
    let full = "ws://localhost:9222/devtools/browser/abc-123-def";
    assert_eq!(normalize_cdp_endpoint(full), full);
}
