//! Regression tests for proxy-error surfacing and retry classification.
//!
//! Locks in the fix for the 2026-04-23 incident where opencode.ai/zen/go
//! returned HTTP 400 with `{"error":{"message":"Provider returned error",
//! "metadata":{"raw":"{\"error\":{\"message\":\"thinking is enabled but
//! reasoning_content is missing in assistant tool call message at index
//! 39\",\"type\":\"invalid_request_error\"}},"provider_name":"Moonshot AI"}}}`
//! — the real Moonshot error was hidden inside `metadata.raw` and we were
//! treating every 400 as non-retryable regardless of content.

use crate::brain::provider::custom_openai_compatible::{
    OpenAIErrorResponse, needs_reasoning_content_for, unwrap_proxy_error,
};
use crate::brain::provider::error::{
    ProviderError, is_html_error_body, is_temporary_unavailable_signal, is_transient_proxy_400,
    is_waf_block_body,
};

// ─── unwrap_proxy_error ─────────────────────────────────────────────

#[test]
fn unwrap_proxy_error_pulls_inner_message_from_opencode_envelope() {
    let body = r#"{
      "error": {
        "message": "Provider returned error",
        "code": 400,
        "metadata": {
          "raw": "{\"error\":{\"message\":\"thinking is enabled but reasoning_content is missing in assistant tool call message at index 39\",\"type\":\"invalid_request_error\"}}",
          "provider_name": "Moonshot AI",
          "is_byok": true
        }
      },
      "user_id": "user_x"
    }"#;
    let parsed: OpenAIErrorResponse = serde_json::from_str(body).expect("parse");
    let (msg, ty) = unwrap_proxy_error(&parsed.error);
    assert_eq!(
        msg,
        "[Moonshot AI] thinking is enabled but reasoning_content is missing in assistant tool call message at index 39"
    );
    assert_eq!(ty.as_deref(), Some("invalid_request_error"));
}

#[test]
fn unwrap_proxy_error_falls_back_when_no_metadata() {
    let body = r#"{"error":{"message":"Missing API key","type":"authentication_error"}}"#;
    let parsed: OpenAIErrorResponse = serde_json::from_str(body).expect("parse");
    let (msg, ty) = unwrap_proxy_error(&parsed.error);
    assert_eq!(msg, "Missing API key");
    assert_eq!(ty.as_deref(), Some("authentication_error"));
}

#[test]
fn unwrap_proxy_error_handles_non_json_raw() {
    let body = r#"{
      "error": {
        "message": "Provider returned error",
        "metadata": {
          "raw": "backend timed out",
          "provider_name": "Alibaba"
        }
      }
    }"#;
    let parsed: OpenAIErrorResponse = serde_json::from_str(body).expect("parse");
    let (msg, _) = unwrap_proxy_error(&parsed.error);
    assert!(msg.contains("[Alibaba]"), "should prefix backend name");
    assert!(
        msg.contains("backend timed out"),
        "should include raw text when it isn't JSON: got {msg:?}"
    );
}

#[test]
fn unwrap_proxy_error_metadata_present_but_no_raw_field() {
    let body = r#"{
      "error": {
        "message": "rate limited",
        "type": "rate_limit_exceeded",
        "metadata": { "provider_name": "Moonshot" }
      }
    }"#;
    let parsed: OpenAIErrorResponse = serde_json::from_str(body).expect("parse");
    let (msg, ty) = unwrap_proxy_error(&parsed.error);
    // No `raw` → return outer as-is (no prefix added).
    assert_eq!(msg, "rate limited");
    assert_eq!(ty.as_deref(), Some("rate_limit_exceeded"));
}

// ─── ProviderError::Display and is_retryable ────────────────────────

#[test]
fn api_error_display_hides_empty_error_type_brackets() {
    let err = ProviderError::ApiError {
        status: 400,
        message: "boom".to_string(),
        error_type: Some(String::new()),
    };
    let rendered = err.to_string();
    assert_eq!(rendered, "API error (400): boom");
    assert!(
        !rendered.contains("[]"),
        "Display must not print '[]' when error_type is Some(\"\")"
    );
}

#[test]
fn api_error_display_shows_non_empty_error_type() {
    let err = ProviderError::ApiError {
        status: 400,
        message: "bad".to_string(),
        error_type: Some("invalid_request_error".to_string()),
    };
    assert_eq!(
        err.to_string(),
        "API error (400) [invalid_request_error]: bad"
    );
}

#[test]
fn transient_proxy_400_retryable_on_generic_passthrough() {
    let err = ProviderError::ApiError {
        status: 400,
        message: "Provider returned error".to_string(),
        error_type: None,
    };
    assert!(
        err.is_retryable(),
        "proxy passthrough 400s must get the retry budget"
    );
}

#[test]
fn transient_proxy_400_retryable_on_empty_type_and_empty_message() {
    let err = ProviderError::ApiError {
        status: 400,
        message: String::new(),
        error_type: Some(String::new()),
    };
    assert!(err.is_retryable());
}

#[test]
fn transient_proxy_400_not_retryable_when_real_error_type_present() {
    let err = ProviderError::ApiError {
        status: 400,
        message:
            "thinking is enabled but reasoning_content is missing in assistant tool call message at index 39"
                .to_string(),
        error_type: Some("invalid_request_error".to_string()),
    };
    assert!(
        !err.is_retryable(),
        "real invalid_request_error must not be retried"
    );
}

#[test]
fn transient_proxy_400_not_retryable_on_specific_client_messages() {
    let err = ProviderError::ApiError {
        status: 400,
        message: "invalid model 'x'".to_string(),
        error_type: None,
    };
    assert!(
        !err.is_retryable(),
        "specific client-side 400 messages stay non-retryable"
    );
}

#[test]
fn is_transient_proxy_400_recognizes_known_phrases() {
    assert!(is_transient_proxy_400("Provider returned error", None));
    assert!(is_transient_proxy_400("Upstream error", Some("")));
    assert!(is_transient_proxy_400("Internal error", None));
    assert!(is_transient_proxy_400("Bad Gateway", Some("")));
    assert!(is_transient_proxy_400("Please try again", None));
    assert!(is_transient_proxy_400("", None));
}

#[test]
fn is_transient_proxy_400_rejects_actionable_messages() {
    assert!(!is_transient_proxy_400(
        "invalid api key format",
        Some("authentication_error")
    ));
    assert!(!is_transient_proxy_400(
        "model 'foo' not found",
        Some("model_not_found")
    ));
    assert!(!is_transient_proxy_400("some random reason", None));
}

// ─── temporary unavailability (overload/capacity) on 4xx JSON (#505) ─
// Some providers report an overloaded/at-capacity model as a 4xx JSON API
// error instead of 429/5xx. Those must be retried in place and (through
// is_retryable) fall to the next provider, WITHOUT reclassifying permanent
// model/auth errors.

#[test]
fn overload_400_with_real_error_type_is_retryable() {
    // A 400 that carries a real error_type would fail is_transient_proxy_400
    // (non-empty type), but the overload wording makes it temporary.
    let err = ProviderError::ApiError {
        status: 400,
        message: "The model is currently overloaded, please try again later".to_string(),
        error_type: Some("invalid_request_error".to_string()),
    };
    assert!(err.is_temporarily_unavailable());
    assert!(
        err.is_retryable(),
        "an overloaded 400 must get the retry budget, not surface"
    );
}

#[test]
fn capacity_409_is_temporary_and_retryable() {
    // A non-400 4xx overload (409) is invisible to is_transient_proxy_400
    // and to should_try_next's 400 arm; the classifier catches it.
    let err = ProviderError::ApiError {
        status: 409,
        message: "Model at capacity".to_string(),
        error_type: Some("capacity".to_string()),
    };
    assert!(err.is_temporarily_unavailable());
    assert!(err.is_retryable());
}

#[test]
fn temporarily_unavailable_503_style_type_on_4xx() {
    let err = ProviderError::ApiError {
        status: 400,
        message: "backend temporarily unavailable".to_string(),
        error_type: Some("server_error".to_string()),
    };
    assert!(err.is_retryable());
}

#[test]
fn permanent_model_not_found_is_not_temporary() {
    // Must NOT be reclassified: routes to the model-mismatch path, not retry.
    let err = ProviderError::ApiError {
        status: 404,
        message: "The model `foo` does not exist or you do not have access".to_string(),
        error_type: Some("model_not_found".to_string()),
    };
    assert!(
        !err.is_temporarily_unavailable(),
        "a permanent model error must never read as temporary"
    );
}

#[test]
fn auth_error_is_not_temporary_even_with_retry_wording() {
    // Belt-and-suspenders: an auth body must stay permanent even if it says
    // "try again".
    let err = ProviderError::ApiError {
        status: 401,
        message: "Invalid API key, please try again with a valid key".to_string(),
        error_type: Some("authentication_error".to_string()),
    };
    assert!(!err.is_temporarily_unavailable());
    // 401 still falls back (dead credential), but via the auth arm, not as a
    // retryable transient error.
    assert!(!err.is_retryable());
}

#[test]
fn is_temporary_unavailable_signal_positive_and_negative() {
    // Overload/capacity/try-again vocabulary → transient.
    assert!(is_temporary_unavailable_signal("Model overloaded", None));
    assert!(is_temporary_unavailable_signal(
        "service unavailable",
        Some("")
    ));
    assert!(is_temporary_unavailable_signal(
        "we are experiencing high demand",
        None
    ));
    assert!(is_temporary_unavailable_signal("please try again", None));
    assert!(is_temporary_unavailable_signal(
        "busy",
        Some("overloaded_error")
    ));
    // Permanent / actionable → not transient.
    assert!(!is_temporary_unavailable_signal(
        "model not found",
        Some("model_not_found")
    ));
    assert!(!is_temporary_unavailable_signal(
        "invalid api key",
        Some("authentication_error")
    ));
    assert!(!is_temporary_unavailable_signal("some unrelated 400", None));
}

// ─── needs_reasoning_content_for ────────────────────────────────────

#[test]
fn reasoning_needed_for_opencode_kimi() {
    assert!(needs_reasoning_content_for(
        "https://opencode.ai/zen/go/v1/chat/completions",
        "kimi-k2.6"
    ));
    assert!(needs_reasoning_content_for(
        "https://opencode.ai/zen/go/v1/chat/completions",
        "Kimi-K2.6"
    ));
}

#[test]
fn reasoning_needed_for_direct_moonshot() {
    assert!(needs_reasoning_content_for(
        "https://api.moonshot.ai/v1/chat/completions",
        "moonshot-v1"
    ));
}

#[test]
fn reasoning_not_needed_for_opencode_qwen() {
    assert!(!needs_reasoning_content_for(
        "https://opencode.ai/zen/go/v1/chat/completions",
        "qwen3.6-plus"
    ));
}

#[test]
fn reasoning_not_needed_for_unrelated_providers() {
    assert!(!needs_reasoning_content_for(
        "https://api.z.ai/api/coding/paas/v4/chat/completions",
        "glm-5.1"
    ));
    assert!(!needs_reasoning_content_for(
        "https://api.minimax.io/v1/chat/completions",
        "MiniMax-M2.7"
    ));
    assert!(!needs_reasoning_content_for(
        "https://api.openai.com/v1/chat/completions",
        "gpt-5"
    ));
}

// ─── HTML error pages on 4xx are transient infra errors (retryable) ──
// Regression (2026-06-07): modelscope intermittently returned HTTP 405
// with a Chinese HTML error page for a valid POST. 405 was non-retryable,
// so the request bounced straight to the fallback chain with ZERO retries
// — the user saw "no resilience, instant fallback" and a manual swap-back
// worked because the 405 was a transient infra blip.

#[test]
fn html_body_detected_as_infra_error() {
    for body in [
        "<!doctypehtml><html lang=\"zh-cn\"><meta charset=\"utf-8\">...",
        "<!DOCTYPE html>\n<html><head><title>405</title></head>",
        "  \n  <html><body>Method Not Allowed</body></html>",
        "<head><title>502 Bad Gateway</title></head>",
    ] {
        assert!(is_html_error_body(body), "should be HTML: {body:?}");
    }
}

#[test]
fn json_body_not_detected_as_infra_error() {
    for body in [
        r#"{"error":{"message":"invalid model","type":"invalid_request_error"}}"#,
        r#"{"error":"unauthorized"}"#,
        "Method Not Allowed",
        "rate limit exceeded, retry in 5s",
    ] {
        assert!(!is_html_error_body(body), "should NOT be HTML: {body:?}");
    }
}

#[test]
fn http_405_with_html_body_is_retryable() {
    // The exact modelscope case: 405 + HTML page → retry, don't instant-fail.
    let err = ProviderError::ApiError {
        status: 405,
        message: "<!doctypehtml><html lang=\"zh-cn\">Method Not Allowed</html>".to_string(),
        error_type: None,
    };
    assert!(
        err.is_retryable(),
        "a 405 with an HTML infra error page must be retryable, not bounced to fallback"
    );
}

#[test]
fn http_405_with_json_body_is_not_retryable() {
    // A genuine API 405 (JSON) is a real client error — do not retry.
    let err = ProviderError::ApiError {
        status: 405,
        message: r#"{"error":{"message":"method not allowed","type":"invalid_request_error"}}"#
            .to_string(),
        error_type: Some("invalid_request_error".to_string()),
    };
    assert!(
        !err.is_retryable(),
        "a JSON 405 client error must stay non-retryable"
    );
}

#[test]
fn http_404_html_retryable_but_json_not() {
    let html = ProviderError::ApiError {
        status: 404,
        message: "<html><head><title>404 Not Found</title></head></html>".to_string(),
        error_type: None,
    };
    assert!(html.is_retryable(), "404 HTML infra page → retryable");

    let json = ProviderError::ApiError {
        status: 404,
        message: r#"{"error":"model not found"}"#.to_string(),
        error_type: None,
    };
    assert!(
        !json.is_retryable(),
        "404 JSON client error → not retryable"
    );
}

// ─── WAF block pages vs transient infra pages ───────────────────────
//
// A 4xx carrying HTML is retried on purpose: a CDN or load-balancer error
// page usually clears on the next attempt. A block page is the opposite —
// the gateway has decided to refuse us, typically at the end of a rate-limit
// escalation, so retrying is futile and risks extending the block.

/// The shape observed on 2026-09-01: an Aliyun block page returned as HTTP
/// 405 for a valid POST, after a week of 429s. Abridged, keeping the markers
/// and their approximate offsets: the wording sits well past the `<style>`
/// block, which is why the check scans further than `is_html_error_body`.
fn aliyun_block_page() -> String {
    format!(
        "<!doctypehtml><html lang=\"zh-cn\"><meta charset=\"utf-8\">\
         <meta name=\"data-spm\"content=\"a3c0e\"><title>405</title>\
         <style>{}</style><body><div id=\"block_message\"></div>\
         <script>var en_tips={{block_message:\"Sorry, your request has been \
         blocked as it may harm the site.\"}}</script></body></html>",
        "a,body,div{margin:0;padding:0}".repeat(20)
    )
}

#[test]
fn an_aliyun_block_page_is_recognised_as_a_block() {
    let body = aliyun_block_page();
    assert!(is_html_error_body(&body), "still an HTML body");
    assert!(is_waf_block_body(&body), "and specifically a block page");
}

/// The marker sits past the 256-char prefix `is_html_error_body` scans, so a
/// short scan would miss it and the page would keep its retry budget.
#[test]
fn the_block_marker_is_found_past_the_html_sniff_prefix() {
    let body = aliyun_block_page();
    let head: String = body.chars().take(256).collect();
    assert!(
        !head.to_ascii_lowercase().contains("has been blocked"),
        "fixture must place the wording past the sniff prefix or it proves nothing"
    );
    assert!(is_waf_block_body(&body));
}

#[test]
fn a_block_page_is_not_retried() {
    let err = ProviderError::ApiError {
        status: 405,
        message: aliyun_block_page(),
        error_type: None,
    };
    assert!(
        !err.is_retryable(),
        "answering a block with the full backoff risks extending it"
    );
}

/// The 2026-06-07 behaviour is preserved: a transient infrastructure page on
/// a 4xx still gets its retries.
#[test]
fn a_transient_html_error_page_is_still_retried() {
    for body in [
        "<!DOCTYPE html>\n<html><head><title>405</title></head></html>",
        "<html><body>502 Bad Gateway</body></html>",
    ] {
        assert!(!is_waf_block_body(body), "not a block page: {body:?}");
        let err = ProviderError::ApiError {
            status: 405,
            message: body.to_string(),
            error_type: None,
        };
        assert!(err.is_retryable(), "should keep its retry budget: {body:?}");
    }
}

#[test]
fn a_json_error_body_is_never_treated_as_a_block() {
    assert!(!is_waf_block_body(
        r#"{"error":{"message":"invalid model","type":"invalid_request_error"}}"#
    ));
}
