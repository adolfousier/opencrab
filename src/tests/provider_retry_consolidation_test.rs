//! Pins the `ProviderError: RetryableError` behaviour after retry
//! consolidation.
//!
//! `brain::provider::retry` was a near-duplicate of `utils::retry`. The
//! consolidation deleted the provider module and routed every provider
//! retry through `utils::retry::retry`, which is generic over
//! `RetryableError`. For that to be behaviour-preserving, `ProviderError`
//! must implement the trait so that:
//!   - `is_retryable()` matches the inherent classifier (transient HTTP /
//!     5xx / rate-limit retry; client 4xx do not), and
//!   - `retry_after()` extracts a server Retry-After hint from rate-limit
//!     errors, clamped to 30s so a pathological "retry after 300s" can't
//!     stall a turn.
//!
//! These tests guard the moved Retry-After parsing and the trait wiring so
//! a future change can't silently break provider retry/fallback timing.

use crate::brain::provider::ProviderError;
use crate::utils::retry::RetryableError;
use std::time::Duration;

#[test]
fn retryable_classification_matches_inherent() {
    // Transient kinds retry.
    assert!(RetryableError::is_retryable(&ProviderError::Timeout(10)));
    assert!(RetryableError::is_retryable(&ProviderError::RateLimitExceeded(
        "slow down".to_string()
    )));
    assert!(RetryableError::is_retryable(&ProviderError::ApiError {
        status: 503,
        message: "upstream unavailable".to_string(),
        error_type: None,
    }));

    // Client errors do not retry.
    assert!(!RetryableError::is_retryable(&ProviderError::InvalidApiKey));
    assert!(!RetryableError::is_retryable(&ProviderError::ApiError {
        status: 400,
        message: "Invalid model id: foo".to_string(),
        error_type: Some("invalid_request_error".to_string()),
    }));

    // Trait and inherent classifiers must agree.
    let e = ProviderError::Timeout(5);
    assert_eq!(RetryableError::is_retryable(&e), e.is_retryable());
}

#[test]
fn retry_after_parses_rate_limit_hint() {
    let e = ProviderError::RateLimitExceeded("retry in 12 seconds".to_string());
    assert_eq!(e.retry_after(), Some(Duration::from_secs(12)));

    let e = ProviderError::ApiError {
        status: 429,
        message: "Too many requests, wait 5s".to_string(),
        error_type: Some("rate_limit".to_string()),
    };
    assert_eq!(e.retry_after(), Some(Duration::from_secs(5)));
}

#[test]
fn retry_after_clamps_to_30s() {
    // A provider asking for an absurd wait must not stall the turn.
    let e = ProviderError::RateLimitExceeded("retry in 300 seconds".to_string());
    assert_eq!(
        e.retry_after(),
        Some(Duration::from_secs(30)),
        "Retry-After hints must be clamped to 30s"
    );
}

#[test]
fn retry_after_none_for_non_rate_limit() {
    // No hint on non-rate-limit errors — caller uses the exponential schedule.
    assert_eq!(ProviderError::Timeout(10).retry_after(), None);
    assert_eq!(ProviderError::InvalidApiKey.retry_after(), None);
    assert_eq!(
        ProviderError::ApiError {
            status: 500,
            message: "boom".to_string(),
            error_type: None,
        }
        .retry_after(),
        None
    );
}

#[test]
fn retry_after_none_when_no_parseable_number() {
    // Rate-limit error with no parseable duration → None (fall back to backoff).
    let e = ProviderError::RateLimitExceeded("you are being rate limited".to_string());
    assert_eq!(e.retry_after(), None);
}

#[tokio::test]
async fn provider_error_drives_generic_retry() {
    // End-to-end: a ProviderError flowing through utils::retry::retry must
    // retry transient errors and stop on non-retryable ones — proving the
    // trait wiring is what the consolidated provider path relies on.
    use crate::utils::retry::{RetryConfig, retry};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    // Fast config so the test doesn't wait the real 1s+ schedule.
    let cfg = RetryConfig {
        max_attempts: 3,
        initial_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(5),
        backoff_multiplier: 2.0,
        jitter: 0.0,
    };

    // Transient: fails twice then succeeds.
    let count = Arc::new(AtomicU32::new(0));
    let c2 = count.clone();
    let out: Result<i32, ProviderError> = retry(
        move || {
            let c = c2.clone();
            async move {
                if c.fetch_add(1, Ordering::SeqCst) < 2 {
                    Err(ProviderError::Timeout(1))
                } else {
                    Ok(7)
                }
            }
        },
        &cfg,
    )
    .await;
    assert_eq!(out.unwrap(), 7);
    assert_eq!(count.load(Ordering::SeqCst), 3, "should retry twice then succeed");

    // Non-retryable: fails once, no retries.
    let count = Arc::new(AtomicU32::new(0));
    let c2 = count.clone();
    let out: Result<i32, ProviderError> = retry(
        move || {
            let c = c2.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Err(ProviderError::InvalidApiKey)
            }
        },
        &cfg,
    )
    .await;
    assert!(out.is_err());
    assert_eq!(count.load(Ordering::SeqCst), 1, "non-retryable must not retry");
}
