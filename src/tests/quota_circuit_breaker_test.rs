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
fn detects_modelscope_this_months_quota_wording() {
    // #1084: modelscope's monthly-dead wording was misclassified as transient.
    // The exact error string from production:
    let msg = "Rate limit exceeded: You have exceeded this month's quota for model \
               Qwen-Ambassador/Qwen3.8-Max, please try again next month, \
               or consider using other models";
    assert!(
        is_quota_exhausted_message(msg),
        "modelscope's 'this month's quota' + 'try again next month' must be hard quota"
    );

    // Also verify with curly apostrophe (Unicode right single quotation mark).
    let curly = msg.replace("month's", "month\u{2019}s");
    assert!(
        is_quota_exhausted_message(&curly),
        "curly apostrophe variant must also be detected"
    );
}

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
// 3a. Per-turn breaker skip (#1084)
// ---------------------------------------------------------------------------

/// Proves that a provider marked mid-turn is immediately invisible to
/// the fallback walk — the breaker does NOT wait for startup or a new
/// session.  When a modelscope quota error fires during a turn, the
/// tool loop calls `mark_exhausted`; the very next `is_exhausted`
/// check (same turn, same millisecond) must skip that provider.
#[test]
fn breaker_mark_skips_provider_per_turn() {
    let _guard = breaker_isolation();
    // Provider starts healthy — would participate in the fallback walk.
    assert!(
        !health::is_exhausted("ModelScope"),
        "provider starts clean (not exhausted)"
    );
    // Simulate mid-turn: a modelscope quota error triggers the breaker.
    health::mark_exhausted("ModelScope");
    // The very next fallback-filter call (same turn) must skip it.
    assert!(
        health::is_exhausted("ModelScope"),
        "provider marked mid-turn must be skipped immediately — no startup gate"
    );
    // Another provider is unaffected.
    assert!(
        !health::is_exhausted("OpenAI"),
        "unrelated provider must not be skipped"
    );
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

// ---------------------------------------------------------------------------
// 4. No-chain setup guidance (#1006) and chain-summary attachment (#1007)
// ---------------------------------------------------------------------------

#[test]
fn no_chain_guidance_points_at_the_exact_fix() {
    // The bare "no fallback providers configured" status taught users
    // nothing. The guidance must name the config block, its shape, the
    // keys requirement, and /restart, or it is useless.
    let g = crate::brain::provider::error::no_chain_setup_guidance();
    assert!(
        g.contains("[providers.fallback]"),
        "must name the config block"
    );
    assert!(g.contains("enabled = true"), "must show the enable switch");
    assert!(g.contains("keys.toml"), "must mention the keys requirement");
    assert!(g.contains("/restart"), "must say how to pick it up");
}

#[test]
fn with_chain_summary_keeps_variant_and_appends_ledger() {
    // #1007: the provider-layer walk must surface the tried ledger without
    // losing the error variant, so upstream classification keeps working.
    let summary = "All providers in the fallback chain failed. x: y.".to_string();

    let e = crate::brain::provider::error::with_chain_summary(
        ProviderError::RateLimitExceeded("rate limited".to_string()),
        summary.clone(),
    );
    match e {
        ProviderError::RateLimitExceeded(m) => {
            assert!(m.starts_with("rate limited"), "original message must lead");
            assert!(
                m.contains("fallback chain failed"),
                "ledger must be attached"
            );
        }
        other => panic!("variant must survive wrapping, got {other:?}"),
    }

    let e = crate::brain::provider::error::with_chain_summary(
        ProviderError::ApiError {
            status: 400,
            message: "invalid_parameter_value".to_string(),
            error_type: Some("http_400".to_string()),
        },
        summary.clone(),
    );
    match e {
        ProviderError::ApiError {
            status,
            message,
            error_type,
        } => {
            assert_eq!(status, 400);
            assert!(message.contains("invalid_parameter_value"));
            assert!(message.contains("fallback chain failed"));
            assert_eq!(error_type.as_deref(), Some("http_400"));
        }
        other => panic!("variant must survive wrapping, got {other:?}"),
    }

    // Variants without a message slot pass through untouched.
    let e = crate::brain::provider::error::with_chain_summary(ProviderError::Timeout(1), summary);
    assert!(matches!(e, ProviderError::Timeout(_)));
}
