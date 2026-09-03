//! Tests for `evolve::diagnose_releases_latest_status` — the
//! honest-error replacement for the prior hardcoded
//! "rate limited or unavailable" suffix that conflated 404
//! (no published release) with 403 (rate limit) with 5xx
//! (GitHub unavailable). One user got "GitHub API returned 404
//! Not Found: rate limited or unavailable" and rationally
//! assumed they'd hit a rate limit when the actual cause was a
//! transient real 404.
//!
//! These tests pin: each branch produces a message that names
//! the actual condition; rate-limit headers are surfaced when
//! present; the body excerpt is appended when the API itself
//! returned an explanation.

use crate::brain::tools::evolve::release_check::diagnose_releases_latest_status;
use reqwest::StatusCode;

#[test]
fn _404_says_no_release_not_rate_limit() {
    let msg = diagnose_releases_latest_status(StatusCode::NOT_FOUND, "", None, None);
    assert!(
        msg.contains("no published"),
        "404 must explain that no published release exists, not lie about rate limit: {msg}"
    );
    assert!(
        !msg.contains("rate limit"),
        "404 must NOT mention rate limit — that was the prior bug: {msg}"
    );
}

#[test]
fn _403_names_rate_limit_and_suggests_token() {
    let msg = diagnose_releases_latest_status(
        StatusCode::FORBIDDEN,
        "API rate limit exceeded",
        Some("0"),
        Some("1717179600"),
    );
    assert!(
        msg.contains("rate limit"),
        "403 must name rate limit: {msg}"
    );
    assert!(
        msg.contains("GITHUB_TOKEN"),
        "403 must suggest the token escape hatch: {msg}"
    );
    assert!(
        msg.contains("x-ratelimit-remaining=0"),
        "403 must surface the remaining quota: {msg}"
    );
    assert!(
        msg.contains("x-ratelimit-reset=1717179600"),
        "403 must surface the reset epoch so the user knows when to retry: {msg}"
    );
    assert!(
        msg.contains("API rate limit exceeded"),
        "403 must quote the API's own error message: {msg}"
    );
}

#[test]
fn _429_treated_same_as_403() {
    let msg = diagnose_releases_latest_status(StatusCode::TOO_MANY_REQUESTS, "", Some("0"), None);
    assert!(
        msg.contains("rate limit"),
        "429 must name rate limit: {msg}"
    );
    assert!(msg.contains("x-ratelimit-remaining=0"));
}

#[test]
fn _5xx_says_transient_server_issue() {
    let msg = diagnose_releases_latest_status(StatusCode::BAD_GATEWAY, "", None, None);
    assert!(
        msg.contains("server-side") || msg.contains("retry"),
        "5xx must signal a transient server-side issue: {msg}"
    );
    assert!(
        !msg.contains("rate limit"),
        "5xx must NOT mention rate limit: {msg}"
    );
}

#[test]
fn unknown_4xx_falls_through_to_generic_with_status() {
    let msg = diagnose_releases_latest_status(StatusCode::UNAUTHORIZED, "", None, None);
    assert!(
        msg.contains("401"),
        "fallthrough must carry the actual status so it's debuggable: {msg}"
    );
}

#[test]
fn body_excerpt_is_appended_when_present() {
    let msg = diagnose_releases_latest_status(
        StatusCode::NOT_FOUND,
        r#"{"message":"Not Found","documentation_url":"..."}"#,
        None,
        None,
    );
    assert!(
        msg.contains("API said"),
        "body excerpt must be appended with a clear lead-in: {msg}"
    );
    assert!(
        msg.contains("\"message\":\"Not Found\""),
        "the actual API message must be quoted verbatim: {msg}"
    );
}

#[test]
fn empty_body_excerpt_omitted_cleanly() {
    let msg = diagnose_releases_latest_status(StatusCode::NOT_FOUND, "   ", None, None);
    assert!(
        !msg.contains("API said"),
        "whitespace-only body must not produce an empty 'API said:' tail: {msg}"
    );
}
