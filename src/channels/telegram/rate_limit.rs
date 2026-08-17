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

/// Longest 429 wait any send path may sleep inline (#1064).
///
/// Telegram can hand out multi-hour windows (8288s observed on a flooded
/// chat). Sleeping the full window inside the send call parked the whole
/// agent turn for hours: the reply was already computed, the process just
/// sat in `tokio::time::sleep` waiting to deliver it. Typical flood windows
/// (placeholder-edit churn, command bursts) are seconds and stay under the
/// cap, so their behavior is unchanged. Oversized windows are slept up to
/// the cap, the retry fails again, and the existing never-silent error
/// paths (#1019) take over. Every send path shares this policy through
/// [`wait_out`].
pub(crate) const MAX_INLINE_RATE_LIMIT_WAIT: Duration = Duration::from_secs(30);

/// The inline wait for a 429: the requested window, clamped to
/// [`MAX_INLINE_RATE_LIMIT_WAIT`]. `capped` tells callers whether the log
/// line should say the wait was shortened (forensics: a capped wait means
/// the chat was flood-banned, not merely throttled).
pub(crate) fn clamp_inline_wait(requested: Duration) -> (Duration, bool) {
    if requested > MAX_INLINE_RATE_LIMIT_WAIT {
        (MAX_INLINE_RATE_LIMIT_WAIT, true)
    } else {
        (requested, false)
    }
}

/// Clamp the requested 429 window and sleep it, logging one standardized
/// line (#1085 P1b R1 — this warn+sleep block was copy-pasted at four send
/// sites with drifting wording). `what` names the send ("HTML send",
/// "edit", ...); `extra` carries per-site context such as an attempt
/// counter or message id into the line. The capped branch's wording is
/// forensic, do not soften it: a window over the cap means the chat is
/// flood-banned, not merely throttled (#1064).
pub(crate) async fn wait_out(what: &str, window: Duration, extra: &str) {
    let (wait, capped) = clamp_inline_wait(window);
    if capped {
        tracing::warn!(
            "Telegram: {what} rate-limited{extra}: {}s window exceeds {}s inline cap \
             — waiting {}s; capped inline, chat likely flood-banned (#1064)",
            window.as_secs(),
            MAX_INLINE_RATE_LIMIT_WAIT.as_secs(),
            wait.as_secs()
        );
    } else {
        tracing::warn!(
            "Telegram: {what} rate-limited{extra} — waiting {}s",
            wait.as_secs()
        );
    }
    tokio::time::sleep(wait).await;
}
