//! Regression tests for the quota-aware retry circuit breaker (#952).
//!
//! Covers three layers:
//! 1. quota-phrase detection in `ProviderError` (so a hard monthly limit is
//!    never confused with a transient throttle),
//! 2. the TTL breaker in `provider::health` (mark / is_exhausted / clear /
//!    TTL expiry),
//! 3. the user-facing `chain_exhausted_summary` and `short_error_reason`.

use crate::brain::provider::error::{
    ProviderError, chain_exhausted_summary, is_quota_exhausted_message, short_error_reason,
};
use crate::brain::provider::health;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

// ---------------------------------------------------------------------------
// 1. Quota detection
// ---------------------------------------------------------------------------

#[test]
fn detects_common_quota_exhaustion_phrases() {
    // The phrases Alexey hit in production (ModelScope / Qwen / Xiaomi monthly
    // caps) plus the OpenAI-style billing messages.
    let cases = [
        "You exceeded your current quota, please check your plan and billing details.",
        "insufficient_quota",
        "You have exhausted your monthly quota.",
        "Your free tier quota has been used up.",
        "You reached your monthly limit.",
        "billing_hard_limit_reached",
    ];
    for msg in cases {
        assert!(
            is_quota_exhausted_message(msg),
            "should detect quota phrase in: {msg}"
        );
    }
}

#[test]
fn does_not_flag_transient_rate_limit_as_quota() {
    // A plain throttle is still retryable; only HARD quotas are not.
    let transient = [
        "Too many requests, slow down",
        "Rate limit exceeded, retry in 32s",
        "Server is busy",
    ];
    for msg in transient {
        assert!(
            !is_quota_exhausted_message(msg),
            "transient throttle must not be treated as quota death: {msg}"
        );
    }
}

#[test]
fn quota_rate_limit_is_not_retryable() {
    // #952 core: a quota RateLimitExceeded must NOT be retried in place.
    let quota = ProviderError::RateLimitExceeded(
        "You exceeded your current quota, please check your plan.".to_string(),
    );
    assert!(quota.is_quota_exhausted());
    assert!(
        !quota.is_retryable(),
        "a hard quota must not burn in-place backoff retries"
    );
}

#[test]
fn quota_429_api_error_is_not_retryable() {
    let quota = ProviderError::ApiError {
        status: 429,
        message: "You have exhausted your monthly quota.".to_string(),
        error_type: None,
    };
    assert!(quota.is_quota_exhausted());
    assert!(!quota.is_retryable());
}

#[test]
fn http_402_payment_required_is_quota_and_not_retryable() {
    // 402 Payment Required reaches the generic catch-all arm (the rate-limit
    // arm only guards 429), so it must still be recognised as quota death.
    let payment = ProviderError::ApiError {
        status: 402,
        message: "Payment required.".to_string(),
        error_type: None,
    };
    assert!(payment.is_quota_exhausted());
    assert!(!payment.is_retryable());
}

#[test]
fn quota_stream_error_is_not_retryable() {
    let quota =
        ProviderError::StreamError("error: you have exhausted your allocated quota".to_string());
    assert!(quota.is_quota_exhausted());
    assert!(!quota.is_retryable());
}

#[test]
fn transient_429_without_quota_phrase_still_retries() {
    // A vanilla throttle (no quota phrase) stays retryable so throttles
    // recover. Transient 429s surface as `RateLimitExceeded` in this codebase
    // (a raw `ApiError { status: 429 }` JSON body was already non-retryable
    // before #952 — it matches no retry arm by design).
    let throttle = ProviderError::RateLimitExceeded("Too many requests, retry later.".to_string());
    assert!(!throttle.is_quota_exhausted());
    assert!(throttle.is_retryable());
}

// ---------------------------------------------------------------------------
// 2. Circuit breaker (provider::health)
// ---------------------------------------------------------------------------

/// Serialize all breaker tests behind one lock AND start from a clean
/// registry. The breaker is process-global and cargo runs tests on parallel
/// threads, so without the lock one test's `mark_exhausted` / `clear_all`
/// races another test's assertions. Bind the returned guard for the whole
/// test body.
static BREAKER_LOCK: Mutex<()> = Mutex::new(());

fn breaker_isolation() -> MutexGuard<'static, ()> {
    let guard = BREAKER_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    health::clear_all();
    guard
}

#[test]
fn breaker_marks_and_reports_exhaustion() {
    let _guard = breaker_isolation();
    assert!(!health::is_exhausted("ModelScope"));
    health::mark_exhausted("ModelScope");
    assert!(
        health::is_exhausted("ModelScope"),
        "marked provider must be reported exhausted"
    );
    assert!(
        health::is_exhausted("modelscope"),
        "breaker key is case-insensitive"
    );
    assert!(health::exhausted_snapshot().contains(&"modelscope".to_string()));
    health::clear_all();
}

#[test]
fn breaker_clear_restores_provider() {
    let _guard = breaker_isolation();
    health::mark_exhausted("Qwen");
    assert!(health::is_exhausted("Qwen"));
    health::clear("Qwen");
    assert!(!health::is_exhausted("Qwen"));
    health::clear_all();
}

#[test]
fn breaker_clear_all_empties_registry() {
    let _guard = breaker_isolation();
    health::mark_exhausted("Qwen");
    health::mark_exhausted("Xiaomi");
    assert_eq!(health::exhausted_snapshot().len(), 2);
    health::clear_all();
    assert!(health::exhausted_snapshot().is_empty());
}

#[test]
fn breaker_short_ttl_expires() {
    let _guard = breaker_isolation();
    // A 1ms TTL must expire; poll a little to let it lapse without a flaky
    // fixed sleep. `is_exhausted` prunes expired entries on read.
    health::mark_exhausted_for("Ephemeral", Duration::from_millis(1));
    assert!(health::is_exhausted("Ephemeral"));
    // Busy-wait up to ~200ms for the 1ms TTL to lapse.
    let mut waited = 0;
    while health::is_exhausted("Ephemeral") && waited < 200 {
        std::thread::sleep(Duration::from_millis(5));
        waited += 5;
    }
    assert!(
        !health::is_exhausted("Ephemeral"),
        "short-TTL mark must expire and be pruned on read"
    );
    health::clear_all();
}

#[test]
fn breaker_ignores_empty_provider_name() {
    let _guard = breaker_isolation();
    health::mark_exhausted("");
    assert!(!health::is_exhausted(""));
    assert!(health::exhausted_snapshot().is_empty());
    health::clear_all();
}

// ---------------------------------------------------------------------------
// 3. User-facing feedback
// ---------------------------------------------------------------------------

#[test]
fn short_reason_distinguishes_quota_from_throttle() {
    let quota = ProviderError::RateLimitExceeded("You exceeded your current quota.".to_string());
    assert_eq!(short_error_reason(&quota), "quota exhausted");

    let throttle = ProviderError::RateLimitExceeded("Slow down.".to_string());
    assert_eq!(short_error_reason(&throttle), "rate limited");

    let timeout = ProviderError::Timeout(5);
    assert_eq!(short_error_reason(&timeout), "timeout");
}

#[test]
fn chain_summary_names_tried_and_skipped_with_hint() {
    let _guard = breaker_isolation();
    let tried = vec![
        "OpenAI/gpt-4o: quota exhausted".to_string(),
        "Anthropic/claude: timeout".to_string(),
    ];
    let skipped = vec!["Qwen".to_string(), "Xiaomi".to_string()];
    let summary = chain_exhausted_summary("ModelScope", "quota exhausted", &tried, &skipped);

    assert!(
        summary.contains("ModelScope: quota exhausted"),
        "names the dead primary + reason"
    );
    assert!(
        summary.contains("OpenAI/gpt-4o: quota exhausted"),
        "lists a tried fallback"
    );
    assert!(summary.contains("Anthropic/claude: timeout"));
    assert!(
        summary.contains("Skipped (quota-exhausted): Qwen, Xiaomi"),
        "lists skipped providers"
    );
    assert!(summary.contains("/models"), "ends with an actionable hint");
    health::clear_all();
}

#[test]
fn chain_summary_without_skipped_omits_section() {
    let tried = vec!["OpenAI/gpt-4o: timeout".to_string()];
    let summary = chain_exhausted_summary("ModelScope", "timeout", &tried, &[]);
    assert!(
        !summary.contains("Skipped"),
        "no skipped section when none skipped"
    );
    assert!(summary.contains("/models"));
}
