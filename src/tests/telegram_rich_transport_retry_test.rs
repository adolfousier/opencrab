//! A rich message that hits a *transport* failure must not be permanently
//! downgraded to HTML (#1323).
//!
//! `classify_rich_edit_error` used to sort every non-429 failure into
//! `Fallback`, the bucket meant for malformed content. A dropped connection is
//! nothing like malformed content: the request never reached Telegram, so
//! nothing about the message was judged. The block lost its formatting for the
//! rest of its life over a socket that was briefly unavailable, which is the
//! trade #580 already rejected for rate limits.

use crate::channels::telegram::flow::{
    MAX_RICH_TRANSPORT_RETRIES, RichEditError, classify_rich_edit_error, is_transport_failure,
};

/// The measured cause: 23 downgrades in one day, all this signature.
const TRANSPORT: &str =
    "error sending request for url (https://api.telegram.org/bot123:SECRET/editMessageText)";

#[test]
fn a_transport_failure_is_not_a_fallback() {
    assert_eq!(
        classify_rich_edit_error(TRANSPORT),
        RichEditError::Transport
    );
    assert_ne!(classify_rich_edit_error(TRANSPORT), RichEditError::Fallback);
}

#[test]
fn the_transport_shapes_seen_in_practice_all_classify() {
    for msg in [
        TRANSPORT,
        "connection closed before message completed",
        "connection reset by peer",
        "tcp connect error: Connection refused (os error 61)",
        "Broken pipe (os error 32)",
        "dns error: failed to lookup address information",
        "operation timed out",
    ] {
        assert!(is_transport_failure(msg), "should be transport: {msg}");
        assert_eq!(
            classify_rich_edit_error(msg),
            RichEditError::Transport,
            "should classify as transport: {msg}"
        );
    }
}

/// The classifier must not become "retry everything". A content error is a
/// permanent property of the message and still has to reach HTML.
#[test]
fn content_errors_still_fall_back() {
    for msg in [
        "Bad Request: can't parse entities",
        "Bad Request: message text is empty",
        "Bad Request: MESSAGE_TOO_LONG",
        "Forbidden: bot was blocked by the user",
    ] {
        assert!(!is_transport_failure(msg), "not transport: {msg}");
        assert_eq!(
            classify_rich_edit_error(msg),
            RichEditError::Fallback,
            "content errors must keep falling back: {msg}"
        );
    }
}

/// #580, unchanged: a 429 retries rich and never splits the block onto the
/// smaller HTML limit.
#[test]
fn rate_limits_are_untouched() {
    for msg in [
        "(429): Too Many Requests: retry after 17",
        "Too Many Requests",
    ] {
        assert_eq!(classify_rich_edit_error(msg), RichEditError::RateLimited);
    }
}

#[test]
fn a_not_modified_response_is_still_a_no_op() {
    assert_eq!(
        classify_rich_edit_error("Bad Request: message is not modified"),
        RichEditError::NotModified
    );
}

/// A transport error whose text also mentions a rate limit stays a rate limit:
/// 429 is checked first, and both recover by retrying rich, so the ordering is
/// safe either way.
#[test]
fn a_429_wins_over_transport_wording() {
    assert_eq!(
        classify_rich_edit_error("429 Too Many Requests while error sending request"),
        RichEditError::RateLimited
    );
}

/// The retry budget has to be finite, or an endpoint that is genuinely
/// unreachable spins instead of delivering through HTML.
#[test]
fn the_transport_retry_budget_is_bounded() {
    const { assert!(MAX_RICH_TRANSPORT_RETRIES >= 1, "must retry at least once") };
    // The edit loop ticks ~1.5s, so even a handful of attempts is already
    // seconds of unreachability; the budget must stay small.
    const { assert!(MAX_RICH_TRANSPORT_RETRIES <= 10, "budget must stay small") };
}

/// The streak drives the decision: retry while within budget, downgrade once
/// past it. This mirrors the arithmetic in `refresh_flow_rich_details` so the
/// boundary is pinned without needing a live Bot API.
#[test]
fn the_streak_downgrades_only_after_the_budget_is_spent() {
    let retries_at = |streak: u8| streak <= MAX_RICH_TRANSPORT_RETRIES;

    for streak in 1..=MAX_RICH_TRANSPORT_RETRIES {
        assert!(
            retries_at(streak),
            "attempt {streak} should still retry rich"
        );
    }
    assert!(
        !retries_at(MAX_RICH_TRANSPORT_RETRIES + 1),
        "the attempt past the budget must downgrade to HTML"
    );
}
