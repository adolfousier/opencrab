//! SSRF guard tests for `web_scrape`. Prove the fetcher refuses every
//! non-public target (localhost, private/loopback/link-local IPs, cloud
//! metadata, non-http schemes) and accepts ordinary public URLs.

use crate::brain::tools::web_scrape::ssrf::validate_url;

#[test]
fn accepts_public_http_and_https() {
    assert!(validate_url("https://example.com/article").is_ok());
    assert!(validate_url("http://example.com").is_ok());
    // 8.8.8.8 is a public IP literal — allowed.
    assert!(validate_url("https://8.8.8.8/").is_ok());
}

#[test]
fn returns_parsed_url_for_relative_base() {
    let parsed = validate_url("https://example.com/blog/post").unwrap();
    // The returned Url is usable as a base for resolving relative links.
    let joined = parsed.join("/img/logo.png").unwrap();
    assert_eq!(joined.as_str(), "https://example.com/img/logo.png");
}

#[test]
fn rejects_non_web_schemes() {
    assert!(validate_url("file:///etc/passwd").is_err());
    assert!(validate_url("ftp://example.com/x").is_err());
    assert!(validate_url("gopher://example.com").is_err());
}

#[test]
fn rejects_localhost() {
    assert!(validate_url("http://localhost/").is_err());
    assert!(validate_url("http://localhost.localdomain/").is_err());
}

#[test]
fn rejects_cloud_metadata_endpoint() {
    assert!(validate_url("http://169.254.169.254/latest/meta-data/").is_err());
    // Any link-local 169.254.0.0/16 host is refused.
    assert!(validate_url("http://169.254.1.1/").is_err());
}

#[test]
fn rejects_private_ipv4_ranges() {
    assert!(validate_url("http://127.0.0.1/").is_err());
    assert!(validate_url("http://10.0.0.5/").is_err());
    assert!(validate_url("http://192.168.1.1/").is_err());
    assert!(validate_url("http://172.16.0.1/").is_err());
}

#[test]
fn rejects_internal_ipv6() {
    assert!(validate_url("http://[::1]/").is_err());
    // Unique-local address (fc00::/7).
    assert!(validate_url("http://[fd00::1]/").is_err());
}

#[test]
fn rejects_garbage() {
    assert!(validate_url("not a url").is_err());
    assert!(validate_url("https://").is_err());
}
