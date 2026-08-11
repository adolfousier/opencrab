//! Provider response ID extraction (#1013).
//!
//! The response id / request-id header is the correlation key that lets an
//! incident be matched against provider-side logs (ModelScope et al). These
//! tests pin the extraction semantics: header family, precedence, and the
//! silent-when-absent contract (providers without request IDs must not
//! produce noise).

use crate::brain::provider::custom_openai_compatible::provider_request_id;
use reqwest::header::{HeaderMap, HeaderValue};

#[test]
fn extracts_x_request_id() {
    let mut h = HeaderMap::new();
    h.insert("x-request-id", HeaderValue::from_static("ms-abc-123"));
    assert_eq!(provider_request_id(&h).as_deref(), Some("ms-abc-123"));
}

#[test]
fn falls_back_through_header_family() {
    let mut h = HeaderMap::new();
    h.insert("request-id", HeaderValue::from_static("r-456"));
    assert_eq!(provider_request_id(&h).as_deref(), Some("r-456"));

    let mut h2 = HeaderMap::new();
    h2.insert("x-trace-id", HeaderValue::from_static("t-789"));
    assert_eq!(provider_request_id(&h2).as_deref(), Some("t-789"));
}

#[test]
fn x_request_id_wins_over_fallbacks() {
    let mut h = HeaderMap::new();
    h.insert("x-request-id", HeaderValue::from_static("primary"));
    h.insert("request-id", HeaderValue::from_static("secondary"));
    assert_eq!(provider_request_id(&h).as_deref(), Some("primary"));
}

#[test]
fn trims_whitespace_around_value() {
    let mut h = HeaderMap::new();
    h.insert("x-request-id", HeaderValue::from_static("  padded-id  "));
    assert_eq!(provider_request_id(&h).as_deref(), Some("padded-id"));
}

#[test]
fn none_when_absent_or_blank() {
    assert_eq!(provider_request_id(&HeaderMap::new()), None);

    let mut h = HeaderMap::new();
    h.insert("x-request-id", HeaderValue::from_static("   "));
    assert_eq!(provider_request_id(&h), None);
}
