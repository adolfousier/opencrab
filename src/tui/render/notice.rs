//! Transient notice on the input box's bottom border (#1369).
//!
//! "Copied to clipboard", "Press Esc again to abort" and the error toast used
//! to be pushed INTO the chat line buffer. Every appearance grew the scroll
//! content by three rows and every expiry shrank it again, so the history
//! jumped on each copy. Nothing else in the TUI uses the chat buffer for
//! chrome, and the input box already carries the ctx budget on that same
//! border, so the notice lives there now: it changes no layout, and it sits
//! where the eye already is while typing.
//!
//! Everything here is pure so the expiry rule, the error-over-info priority
//! and the single-row squeeze are unit-testable without a live `App`.

use std::time::{Duration, Instant};

/// How long a non-error notice ("Copied to clipboard") stays on the border.
pub(crate) const NOTIFICATION_TTL: Duration = Duration::from_secs(2);

/// How long a timestamped error toast stays on the border.
pub(crate) const ERROR_TTL: Duration = Duration::from_millis(2500);

/// Which style the border slot takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NoticeKind {
    Error,
    Info,
}

/// True once a timestamped notice has outlived `ttl`.
///
/// A notice with no timestamp never expires here: errors raised by the
/// runner (render panics, event-loop failures) carry no `shown_at` and are
/// meant to stay until the user acts, exactly as before this module existed.
pub(crate) fn notice_expired(shown_at: Option<Instant>, now: Instant, ttl: Duration) -> bool {
    shown_at.is_some_and(|t| now.saturating_duration_since(t) >= ttl)
}

/// One slot, two candidates: an error outranks an informational notice.
pub(crate) fn pick_notice<'a>(
    error: Option<&'a str>,
    info: Option<&'a str>,
) -> Option<(NoticeKind, &'a str)> {
    error
        .map(|e| (NoticeKind::Error, e))
        .or_else(|| info.map(|i| (NoticeKind::Info, i)))
}

/// Squeeze `text` into a single border row of at most `max_chars` cells.
///
/// Newlines and whitespace runs collapse to one space, the result is padded
/// with one space either side, and anything longer than the slot is cut with
/// an ellipsis. Multi-line errors keep their full text in the permanent
/// error bubble in the chat; the border only has to say that one fired.
/// Returns `None` when the slot cannot hold even one character plus the
/// ellipsis and padding.
pub(crate) fn notice_label(text: &str, max_chars: usize) -> Option<String> {
    const PADDING: usize = 2;
    const ELLIPSIS: char = '…';
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() || max_chars < PADDING + 2 {
        return None;
    }
    let room = max_chars - PADDING;
    let len = collapsed.chars().count();
    let body: String = if len <= room {
        collapsed
    } else {
        let mut cut: String = collapsed.chars().take(room - 1).collect();
        cut.push(ELLIPSIS);
        cut
    };
    Some(format!(" {body} "))
}
