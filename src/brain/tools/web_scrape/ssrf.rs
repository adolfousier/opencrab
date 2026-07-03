//! SSRF guard for `web_scrape`.
//!
//! Ported from insight_forge's `url_validator`. A URL-to-markdown tool is a
//! server-side fetcher, so an unguarded fetch is a classic SSRF primitive:
//! point it at `http://169.254.169.254/…` or `http://localhost:6379/` and it
//! happily reaches into the host's network. This rejects anything that isn't a
//! plain public `http(s)` resource before a single byte goes out.
//!
//! Host classification goes through [`url::Host`] rather than parsing
//! `host_str()`, because `host_str()` keeps the brackets on IPv6 literals
//! (`[::1]`), which then fail `IpAddr` parsing and slip past the filter.

use url::{Host, Url};

/// Validate that `url` is safe to fetch. Returns the parsed [`Url`] on success
/// (handy as a base for resolving relative links later), or a human-readable
/// reason on rejection.
pub fn validate_url(url: &str) -> Result<Url, String> {
    let parsed = Url::parse(url).map_err(|e| format!("invalid URL: {e}"))?;

    // Only plain web schemes. `file://` in particular would let the tool read
    // local files, so it is called out explicitly.
    match parsed.scheme() {
        "http" | "https" => {}
        "file" => return Err("file:// URLs are not allowed".to_string()),
        other => return Err(format!("unsupported scheme: {other}")),
    }

    match parsed.host() {
        None => return Err("URL has no host".to_string()),
        Some(Host::Domain(host)) => {
            let lower = host.to_ascii_lowercase();
            if lower == "localhost" || lower == "localhost.localdomain" {
                return Err("requests to localhost are not allowed".to_string());
            }
        }
        Some(Host::Ipv4(v4)) => {
            // Link-local (169.254.0.0/16) also covers the cloud metadata
            // endpoint 169.254.169.254.
            if v4.is_private() || v4.is_loopback() || v4.is_link_local() || v4.is_broadcast() {
                return Err("requests to private/internal IP addresses are not allowed".to_string());
            }
            if v4.is_multicast() {
                return Err("requests to multicast addresses are not allowed".to_string());
            }
        }
        Some(Host::Ipv6(v6)) => {
            if v6.is_loopback() || v6.is_multicast() {
                return Err("requests to internal IPv6 addresses are not allowed".to_string());
            }
            // Unique-local addresses (fc00::/7) are the IPv6 analogue of the
            // IPv4 private ranges.
            if (v6.segments()[0] & 0xfe00) == 0xfc00 {
                return Err("requests to private IPv6 addresses are not allowed".to_string());
            }
        }
    }

    Ok(parsed)
}
