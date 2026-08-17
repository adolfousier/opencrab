//! Send-correlation telemetry for the Telegram surface (#1085 P1a).
//!
//! Duplicate-send and chatty-agent investigations need to attribute every
//! message that LANDS in a chat to its origin. The twin audits
//! (2026-08-17) found exactly one fully-logged send site out of ~57 groups:
//! success paths were log-invisible almost everywhere.
//!
//! P1a policy (grill round 1, decisions Q2/Q3): log METADATA only
//! `{chat_id, thread_id, message_id, kind, path, origin, len, hash8}` —
//! never message content. The hash lets an investigation correlate "same
//! text sent twice" without the log ever carrying the text itself.
//!
//! `origin` is a closed vocabulary, `turn | tool | cron | system`; free
//! text goes in `origin_detail` at the call site, never here.

use sha2::{Digest, Sha256};

/// First 8 hex chars of the SHA-256 of `content`.
///
/// 32 bits of hash is ample to correlate duplicates within a day's log
/// (collision odds are irrelevant at log volumes); sha2 is already a
/// direct dependency, so no new crate rides along.
pub(crate) fn content_hash8(content: &str) -> String {
    let digest = Sha256::digest(content.as_bytes());
    digest[..4]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join("")
}

/// Log a message that landed in a chat. One grep shape for every send
/// site on the surface: `grep "Telegram send ok:"`.
#[allow(clippy::too_many_arguments)] // correlation fields per the #1085 P1a schema
pub(crate) fn log_send_success(
    origin: &str,
    kind: &str,
    path: &str,
    chat_id: i64,
    thread_id: Option<i32>,
    msg_id: i32,
    len: usize,
    hash8: &str,
) {
    tracing::debug!(
        "Telegram send ok: origin={origin} kind={kind} path={path} \
         chat={chat_id} thread={thread_id:?} msg={msg_id} len={len} hash8={hash8}"
    );
}

/// Log a send that failed on every path it tried. The `error` string is
/// the transport error (never message content).
#[allow(clippy::too_many_arguments)] // correlation fields per the #1085 P1a schema
pub(crate) fn log_send_failure(
    origin: &str,
    kind: &str,
    path: &str,
    chat_id: i64,
    thread_id: Option<i32>,
    len: usize,
    hash8: &str,
    error: &str,
) {
    tracing::warn!(
        "Telegram send failed: origin={origin} kind={kind} path={path} \
         chat={chat_id} thread={thread_id:?} len={len} hash8={hash8} error={error}"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash8_is_stable_and_8_hex_chars() {
        let a = content_hash8("hello world");
        let b = content_hash8("hello world");
        assert_eq!(a, b);
        assert_eq!(a.len(), 8);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn hash8_separates_different_content() {
        assert_ne!(content_hash8("hello world"), content_hash8("hello worlD"));
    }

    #[test]
    fn hash8_handles_empty_and_multibyte() {
        assert_eq!(content_hash8("").len(), 8);
        // PT-PT and emoji content must not panic (multibyte boundary safety).
        assert_eq!(content_hash8("ção 🦀 açúcar").len(), 8);
    }
}
