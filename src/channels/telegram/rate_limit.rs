//! Back off when Telegram says to (#814).
//!
//! The plan card was writing often enough to trip flood control, and the error
//! was logged and dropped. Nothing recorded that the API had asked for a pause,
//! so the next refresh wrote again immediately and each rejected attempt kept
//! the window alive. Observed as roughly twenty create attempts across forty
//! seconds while the countdown ticked 40s down to 3s without ever elapsing.
//!
//! The card is chrome. Skipping an update is strictly better than being
//! throttled into a loop that also spams duplicates into the chat.

use std::time::Duration;

/// Seconds Telegram asked us to wait, from an error string.
///
/// teloxide surfaces flood control as text containing `Retry after N`, so this
/// matches on that rather than a typed variant, which keeps it working across
/// the several error shapes the same condition arrives in.
///
/// Returns `None` for anything else, so ordinary failures (a deleted message,
/// bad markup) are not mistaken for throttling and do not suppress writes.
pub(crate) fn parse_retry_after(error: &str) -> Option<Duration> {
    let idx = error.find("Retry after")?;
    let rest = &error[idx + "Retry after".len()..];
    let digits: String = rest
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(char::is_ascii_digit)
        .collect();
    if digits.is_empty() {
        return None;
    }
    let secs: u64 = digits.parse().ok()?;
    // A pause is only meaningful if it is positive; "Retry after 0" is not a
    // reason to stop writing.
    (secs > 0).then(|| Duration::from_secs(secs))
}

/// Extra margin added to the window Telegram gives.
///
/// Resuming on the exact second risks landing inside the same window and
/// renewing the penalty, which is the loop this exists to break.
pub(crate) const RETRY_MARGIN: Duration = Duration::from_secs(2);
