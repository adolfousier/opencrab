//! Send-correlation telemetry for the Telegram surface (#1085 P1a).
//!
//! Duplicate-send and chatty-agent investigations need to attribute every
//! message that LANDS in a chat to its origin. The twin audits
//! (2026-08-17) found exactly one fully-logged send site out of ~57 groups:
//! success paths were log-invisible almost everywhere.
//!
//! P1a policy (grill round 1, decisions Q2/Q3; review F3/F7/F9): log
//! METADATA only — `{chat_id, thread_id, message_id, kind, path, origin,
//! origin_detail, session, len, hash8}` — never message content. The hash
//! lets an investigation correlate "same text sent twice" without the log
//! ever carrying the text itself. Success lines are `info!`: production
//! daemons run at INFO, and an invisible attribution line defeats the
//! purpose.
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
///
/// `info!` (not debug): production daemons run at INFO, and an
/// attribution line that is invisible by default defeats the audit's
/// whole purpose (review F7). `origin_detail` carries the job/arm name
/// (Q3), `session` the originating session id or "-" where the call
/// site genuinely cannot know it (review F9).
#[allow(clippy::too_many_arguments)] // correlation fields per the #1085 P1a schema
pub(crate) fn log_send_success(
    origin: &str,
    origin_detail: &str,
    session: &str,
    kind: &str,
    path: &str,
    chat_id: i64,
    thread_id: Option<i32>,
    msg_id: i32,
    len: usize,
    hash8: &str,
) {
    tracing::info!(
        "Telegram send ok: origin={origin} detail={origin_detail} session={session} \
         kind={kind} path={path} chat={chat_id} thread={thread_id:?} msg={msg_id} len={len} \
         hash8={hash8}"
    );
}

/// Log a send that failed on every path it tried. The `error` string is
/// the transport error (never message content).
#[allow(clippy::too_many_arguments)] // correlation fields per the #1085 P1a schema
pub(crate) fn log_send_failure(
    origin: &str,
    origin_detail: &str,
    session: &str,
    kind: &str,
    path: &str,
    chat_id: i64,
    thread_id: Option<i32>,
    len: usize,
    hash8: &str,
    error: &str,
) {
    tracing::warn!(
        "Telegram send failed: origin={origin} detail={origin_detail} session={session} \
         kind={kind} path={path} chat={chat_id} thread={thread_id:?} len={len} hash8={hash8} \
         error={error}"
    );
}
