//! Regression test for Telegram long rate-limit guard (#1110).
//!
//! When Telegram returns `Retry-After: N` where N > 1 hour, the chat is
//! flood-banned for hours (28442s = 7.9 hours observed in Adi's audit).
//! Retrying the send ladder burns 90 seconds (3 × 30s clamped wait) for
//! no gain. This test pins the fix: long rate-limits bail immediately.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use teloxide::types::Seconds;
use teloxide::RequestError;

/// Long rate-limit (>1 hour) bails immediately without retrying.
#[tokio::test]
async fn long_rate_limit_bails_immediately() {
    let attempts = Arc::new(AtomicU32::new(0));
    let attempts_clone = attempts.clone();

    // Mock a send that always returns a 7.9-hour rate-limit
    let result = crate::channels::telegram::intermediates::send_retrying_rate_limit(
        "test send",
        move || {
            let attempts = attempts_clone.clone();
            async move {
                attempts.fetch_add(1, Ordering::SeqCst);
                // 28442 seconds = 7.9 hours (observed in Adi's audit)
                Err::<(), _>(RequestError::RetryAfter(Seconds::from_seconds(28442)))
            }
        },
    )
    .await;

    // Should fail immediately without retrying
    assert!(result.is_err(), "Long rate-limit should return error");
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        1,
        "Long rate-limit should bail after 1 attempt, not retry"
    );
}

/// Short rate-limit (<1 hour) retries normally up to 3 attempts.
#[tokio::test]
async fn short_rate_limit_retries_normally() {
    let attempts = Arc::new(AtomicU32::new(0));
    let attempts_clone = attempts.clone();

    // Mock a send that returns a 30-second rate-limit 3 times, then succeeds
    let result = crate::channels::telegram::intermediates::send_retrying_rate_limit(
        "test send",
        move || {
            let attempts = attempts_clone.clone();
            async move {
                let count = attempts.fetch_add(1, Ordering::SeqCst);
                if count < 3 {
                    // 30 seconds (typical flood window)
                    Err::<(), _>(RequestError::RetryAfter(Seconds::from_seconds(30)))
                } else {
                    Ok(())
                }
            }
        },
    )
    .await;

    // Should succeed after retries
    assert!(result.is_ok(), "Short rate-limit should succeed after retries");
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        4,
        "Short rate-limit should make 4 attempts (1 initial + 3 retries)"
    );
}

/// Rate-limit at exactly 1 hour (3600s) is NOT long yet (boundary check).
#[tokio::test]
async fn rate_limit_at_threshold_retries() {
    let attempts = Arc::new(AtomicU32::new(0));
    let attempts_clone = attempts.clone();

    // Mock a send that returns exactly 3600s (1 hour) rate-limit
    let result = crate::channels::telegram::intermediates::send_retrying_rate_limit(
        "test send",
        move || {
            let attempts = attempts_clone.clone();
            async move {
                attempts.fetch_add(1, Ordering::SeqCst);
                // Exactly 1 hour (at threshold, not over)
                Err::<(), _>(RequestError::RetryAfter(Seconds::from_seconds(3600)))
            }
        },
    )
    .await;

    // Should retry (not bail immediately) because it's AT threshold, not OVER
    assert!(result.is_err(), "Should fail after exhausting retries");
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        4,
        "Rate-limit at threshold should make 4 attempts (1 initial + 3 retries)"
    );
}
