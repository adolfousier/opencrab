//! Backing off when Telegram says to (#814, regression from 3c45e41a).
//!
//! The card was re-stuck (deleted and reposted at the bottom) on every settle,
//! which cost a delete plus a create per turn on top of the refreshes already
//! firing from the streaming path. That put it within reach of flood control,
//! and the error was logged and dropped, so the next refresh wrote again
//! immediately and renewed the window. Observed as ~20 create attempts across
//! 40 seconds while the countdown ticked 40s down to 3s without elapsing,
//! leaving four duplicate cards in the chat.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::channels::telegram::rate_limit::parse_retry_after;
use std::time::Duration;

#[test]
fn a_flood_control_error_yields_its_wait() {
    // The exact shape seen in the logs.
    assert_eq!(
        parse_retry_after("Telegram(RetryAfter): Retry after 40s"),
        Some(Duration::from_secs(40))
    );
}

#[test]
fn the_wait_is_read_whatever_surrounds_it() {
    // teloxide surfaces this condition in several error shapes, so matching
    // must not depend on the wrapping text.
    for text in [
        "Retry after 3",
        "ApiError: Too Many Requests: Retry after 12",
        "error sending request: Retry after 7s (flood control)",
    ] {
        assert!(
            parse_retry_after(text).is_some(),
            "must recognise a wait in: {text}"
        );
    }
}

#[test]
fn an_ordinary_failure_is_not_throttling() {
    // Mistaking these for a throttle would suppress the card for a genuine
    // error it could have recovered from immediately.
    for text in [
        "message to edit not found",
        "message is not modified",
        "Bad Request: can't parse entities",
        "",
    ] {
        assert_eq!(
            parse_retry_after(text),
            None,
            "must not suppress on: {text}"
        );
    }
}

#[test]
fn a_zero_wait_is_not_a_reason_to_stop() {
    assert_eq!(parse_retry_after("Retry after 0"), None);
    assert_eq!(parse_retry_after("Retry after 0s"), None);
}

#[test]
fn the_margin_pushes_past_the_window() {
    // Resuming on the exact second risks landing inside the same window and
    // renewing the penalty, which is the loop being broken here.
    use crate::channels::telegram::rate_limit::RETRY_MARGIN;
    assert!(
        RETRY_MARGIN > Duration::ZERO,
        "resuming exactly on the boundary re-triggers the window"
    );
}
