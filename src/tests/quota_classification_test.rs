//! Quota classification and the user-facing chain summary.
//!
//! Classification exists so a hard monthly limit is not retried in place like
//! a transient throttle: the chain advances immediately instead of burning
//! three retries. It informs the human and shapes the error text. It must
//! never route — the TTL quarantine registry this file was originally written
//! for (`provider::health`, #952) was deleted in #1251 because it removed
//! providers from consideration behind the user's back.
//!
//! Covers:
//! 1. quota-phrase detection in `ProviderError`,
//! 2. the user-facing `chain_exhausted_summary` and `short_error_reason`.

use crate::brain::provider::error::{
    ProviderError, chain_exhausted_summary, is_quota_exhausted_message, short_error_reason,
};

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
fn chain_summary_names_every_provider_tried_with_a_hint() {
    let tried = vec![
        "OpenAI/gpt-4o: quota exhausted".to_string(),
        "Anthropic/claude: timeout".to_string(),
    ];
    let summary = chain_exhausted_summary("ModelScope", "quota exhausted", &tried);

    assert!(
        summary.contains("ModelScope: quota exhausted"),
        "names the dead primary + reason"
    );
    assert!(
        summary.contains("OpenAI/gpt-4o: quota exhausted"),
        "lists a tried fallback"
    );
    assert!(summary.contains("Anthropic/claude: timeout"));
    assert!(summary.contains("/models"), "ends with an actionable hint");
}

#[test]
fn chain_summary_never_reports_a_skipped_provider() {
    // #1251: nothing is ever skipped, so the summary must not carry the
    // concept. A "Skipped" line would mean a provider was silently dropped
    // from the walk — exactly the behaviour that was removed.
    let tried = vec!["OpenAI/gpt-4o: timeout".to_string()];
    let summary = chain_exhausted_summary("ModelScope", "timeout", &tried);
    assert!(
        !summary.contains("Skipped"),
        "no provider is ever skipped, so no skipped section can exist"
    );
    assert!(summary.contains("/models"));
}

// ---------------------------------------------------------------------------
// 3. No-chain setup guidance (#1006) and chain-summary attachment (#1007)
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
