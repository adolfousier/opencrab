//! Error types for LLM providers

use thiserror::Error;

/// Provider error types
#[derive(Debug, Error)]
pub enum ProviderError {
    /// HTTP request failed
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),

    /// API returned an error
    #[error(
        "API error ({status}){}: {message}",
        error_type
            .as_ref()
            .filter(|t| !t.is_empty())
            .map(|t| format!(" [{}]", t))
            .unwrap_or_default()
    )]
    ApiError {
        status: u16,
        message: String,
        error_type: Option<String>,
    },

    /// Invalid API key
    #[error("Invalid API key")]
    InvalidApiKey,

    /// Rate limit exceeded
    #[error("Rate limit exceeded: {0}")]
    RateLimitExceeded(String),

    /// Invalid request
    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    /// Model not found
    #[error("Model not found: {0}")]
    ModelNotFound(String),

    /// Context length exceeded
    #[error("Context length exceeded: {0} tokens")]
    ContextLengthExceeded(u32),

    /// Streaming not supported
    #[error("Streaming not supported by this provider")]
    StreamingNotSupported,

    /// Tools not supported
    #[error("Tools not supported by this provider")]
    ToolsNotSupported,

    /// JSON parsing error
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    /// Streaming error
    #[error("Streaming error: {0}")]
    StreamError(String),

    /// Timeout
    #[error("Request timed out after {0}s")]
    Timeout(u64),

    /// Internal error
    #[error("Internal error: {0}")]
    Internal(String),
}

impl ProviderError {
    /// Check if error is retryable
    pub fn is_retryable(&self) -> bool {
        match self {
            ProviderError::HttpError(_)
            | ProviderError::RateLimitExceeded(_)
            | ProviderError::Timeout(_)
            // A stream that broke mid-flight ("connection closed before message
            // completed", the SSE socket dropping, a partial body) is a
            // transport hiccup, not a client mistake — re-issuing the request
            // usually succeeds. Retry it like the other transport errors instead
            // of bouncing straight to the fallback chain. A genuinely fatal
            // cause (bad model, auth, invalid content) surfaces as a typed
            // ApiError with its own status, which is handled below — it never
            // reaches here as a StreamError.
            | ProviderError::StreamError(_) => true,
            ProviderError::ApiError { status, .. } if *status >= 500 => true,
            // A 4xx whose body is an HTML page is an infrastructure / CDN /
            // load-balancer error page, NOT a real JSON API client error.
            // These are transient (the next request usually hits a healthy
            // node) and must be retried, not bounced straight to the
            // fallback chain. Canonical case: modelscope intermittently
            // returns HTTP 405 with a Chinese HTML error page for a valid
            // POST to /chat/completions; retrying succeeds, but the old code
            // treated 405 as a hard client error and fell back instantly
            // with zero retries (2026-06-07). Real client errors return
            // JSON, never HTML, so this never masks an invalid_model /
            // validation / auth problem.
            ProviderError::ApiError {
                status, message, ..
            } if (400..500).contains(status) && is_html_error_body(message) => true,
            // HTTP 400 with a generic proxy-style body (empty error_type
            // AND a message that doesn't describe an actionable client
            // problem) is almost always a transient upstream failure
            // forwarded by the proxy. opencode.ai's "Provider returned
            // error" is the canonical case — the user's payload is fine,
            // their upstream is having a moment. Retry before falling
            // back. Real client-side 400s (invalid_model, validation
            // errors, bad JSON) carry specific error_type or message
            // strings and stay non-retryable.
            ProviderError::ApiError {
                status: 400,
                message,
                error_type,
            } => is_transient_proxy_400(message, error_type.as_deref()),
            _ => false,
        }
    }

    /// Get HTTP status code if available
    pub fn status_code(&self) -> Option<u16> {
        match self {
            ProviderError::ApiError { status, .. } => Some(*status),
            _ => None,
        }
    }

    /// True when the server rejected the REQUEST's model id (not the
    /// credential). Some OpenAI-compatible proxies — notably
    /// `opencode.ai/zen` — return HTTP 401 with
    /// `{"error":{"type":"ModelError","message":"Model X not supported"}}`
    /// for "this key can't use that model", which collides with real
    /// auth failures. Downstream code uses this to keep the actual
    /// "invalid key" classification meaningful and route model-mismatch
    /// errors to a different UX path.
    pub fn is_model_unsupported(&self) -> bool {
        match self {
            ProviderError::ModelNotFound(_) => true,
            ProviderError::ApiError {
                error_type,
                message,
                ..
            } => {
                let type_hit = error_type.as_ref().is_some_and(|t| {
                    let t = t.to_ascii_lowercase();
                    t == "modelerror"
                        || t == "model_error"
                        || t == "model_not_found"
                        || t == "invalid_model"
                });
                let msg = message.to_ascii_lowercase();
                let msg_hit = msg.contains("model")
                    && (msg.contains("not supported")
                        || msg.contains("not found")
                        || msg.contains("unsupported"));
                type_hit || msg_hit
            }
            _ => false,
        }
    }
}

/// True when an error body is an HTML page rather than a JSON API error.
/// A 4xx that returns HTML came from a CDN / load balancer / reverse proxy
/// (an infrastructure error page), not the API itself — these are
/// transient and worth retrying. Real API client errors are always JSON,
/// so this never matches a genuine invalid_model / validation / auth error.
pub(crate) fn is_html_error_body(message: &str) -> bool {
    // Scan a bounded prefix so a huge HTML page isn't lowercased in full on
    // every error. `chars().take()` is char-boundary-safe — a byte slice
    // would panic mid-UTF8 (the modelscope body has Chinese characters).
    let head: String = message
        .trim_start()
        .chars()
        .take(256)
        .collect::<String>()
        .to_ascii_lowercase();
    head.contains("<!doctype")
        || head.contains("<html")
        || head.contains("<head")
        || head.contains("<body")
}

/// True when an HTTP 400 response body looks like a proxy passthrough of
/// an upstream hiccup rather than a real client-side error. Used by
/// `is_retryable` so opencode.ai-style "Provider returned error" 400s
/// go through the 3-retry backoff instead of bailing to fallback on
/// the first try.
pub(crate) fn is_transient_proxy_400(message: &str, error_type: Option<&str>) -> bool {
    // Real client errors always carry an error_type (OpenAI: "invalid_request_error",
    // "model_not_found", "validation_error", etc.). Treat any non-empty type as
    // non-transient so we don't retry bad payloads.
    if error_type.is_some_and(|t| !t.is_empty()) {
        return false;
    }
    let m = message.trim().to_ascii_lowercase();
    if m.is_empty() {
        return true;
    }
    // Known proxy-passthrough phrases. Add new strings here when a proxy
    // invents a different one.
    const TRANSIENT_HINTS: &[&str] = &[
        "provider returned error",
        "upstream error",
        "internal error",
        "temporary",
        "try again",
        "bad gateway",
    ];
    TRANSIENT_HINTS.iter().any(|h| m.contains(h))
}

/// Result type for provider operations
pub type Result<T> = std::result::Result<T, ProviderError>;

impl crate::utils::retry::RetryableError for ProviderError {
    fn is_retryable(&self) -> bool {
        // Delegate to the inherent classifier. `Self::is_retryable` would
        // be ambiguous (inherent vs this trait method), so go through a
        // free helper that names the inherent unambiguously.
        provider_error_is_retryable(self)
    }

    fn retry_after(&self) -> Option<std::time::Duration> {
        // Parse a server Retry-After hint from rate-limit errors, clamped
        // to 30s so a pathological "retry after 300s" can't stall a turn.
        // Other error kinds have no hint — the caller falls back to the
        // exponential schedule.
        let msg = match self {
            ProviderError::RateLimitExceeded(m) => m.as_str(),
            ProviderError::ApiError {
                status, message, ..
            } if *status == 429 => message.as_str(),
            _ => return None,
        };
        parse_retry_seconds(msg).map(|secs| std::time::Duration::from_secs(secs.min(30)))
    }
}

/// Free wrapper so the `RetryableError` impl can call the inherent
/// `ProviderError::is_retryable` without method-resolution ambiguity.
fn provider_error_is_retryable(e: &ProviderError) -> bool {
    e.is_retryable()
}

/// Render a concise, SPECIFIC reason for a provider error, for the user-facing
/// TUI warnings ("⏳ Retry…", "🔧 Switched to…"). For HTTP errors this digs
/// through reqwest's source chain to the real cause — DNS lookup failure,
/// connection refused, TLS error, timeout — instead of the opaque top-level
/// "error sending request for url (…)" that hides what actually happened.
pub fn user_facing_reason(err: &ProviderError) -> String {
    match err {
        ProviderError::HttpError(e) => describe_reqwest_error(e),
        other => other.to_string(),
    }
}

/// Classify a reqwest error into a short, specific phrase by walking its source
/// chain to the deepest OS/resolver cause. Appends the host when known, e.g.
/// "DNS lookup failed (www.dialagram.me)" or "connection refused (api.x.com)".
pub(crate) fn describe_reqwest_error(e: &reqwest::Error) -> String {
    let host_suffix = e
        .url()
        .and_then(|u| u.host_str())
        .map(|h| format!(" ({h})"))
        .unwrap_or_default();

    if e.is_timeout() {
        return format!("request timed out{host_suffix}");
    }

    // The deepest source-chain entry carries the real OS/resolver error; the
    // reqwest top-level Display ("error sending request for url …") does not.
    let mut deepest: Option<String> = None;
    let mut src: Option<&(dyn std::error::Error + 'static)> = std::error::Error::source(e);
    while let Some(s) = src {
        deepest = Some(s.to_string());
        src = s.source();
    }
    let detail = deepest.unwrap_or_else(|| e.to_string());
    let low = detail.to_ascii_lowercase();

    let label = if low.contains("dns")
        || low.contains("lookup address")
        || low.contains("nodename nor servname")
        || low.contains("name or service not known")
        || low.contains("no such host")
        || low.contains("failed to resolve")
        || low.contains("could not resolve")
    {
        "DNS lookup failed"
    } else if low.contains("connection refused") {
        "connection refused"
    } else if low.contains("connection reset") {
        "connection reset by peer"
    } else if low.contains("network is unreachable") {
        "network unreachable"
    } else if low.contains("no route to host") {
        "no route to host"
    } else if low.contains("timed out") || low.contains("timeout") {
        "timed out"
    } else if low.contains("certificate")
        || low.contains("tls")
        || low.contains("ssl")
        || low.contains("handshake")
    {
        "TLS/certificate error"
    } else {
        // Unknown shape — surface the real deepest cause itself, trimmed, so
        // we never hide what happened behind a generic label.
        let trimmed: String = detail.chars().take(140).collect();
        return format!("{trimmed}{host_suffix}");
    };
    format!("{label}{host_suffix}")
}

/// Parse a retry-delay (seconds) out of a rate-limit error message.
/// Recognizes "60 seconds", "60s", "retry in 60", "wait 60". Moved here
/// from the former `brain::provider::retry` module when retry logic was
/// consolidated onto `utils::retry`.
fn parse_retry_seconds(msg: &str) -> Option<u64> {
    use regex::Regex;
    let patterns = [
        r"(\d+)\s*seconds?",
        r"(\d+)\s*s\b",
        r"retry in (\d+)",
        r"wait (\d+)",
    ];
    for pattern in patterns {
        if let Ok(re) = Regex::new(pattern)
            && let Some(captures) = re.captures(msg)
            && let Some(num_str) = captures.get(1)
            && let Ok(secs) = num_str.as_str().parse::<u64>()
        {
            return Some(secs);
        }
    }
    None
}

#[cfg(test)]
#[path = "../../tests/brain_provider_error_test.rs"]
mod tests;
