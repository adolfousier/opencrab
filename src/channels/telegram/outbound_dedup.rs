//! Dedup of outbound media the agent uploads via `telegram_send` (#721).
//!
//! A model can repeat a `send_photo`/`send_document` call, and a large upload
//! can time out client-side after Telegram already delivered it and then get
//! re-sent — both land the identical file+caption twice back-to-back. Claiming
//! a signature suppresses an identical send within a short window while still
//! allowing a deliberate re-send later.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Default)]
pub(crate) struct MediaSendDedup {
    seen: Mutex<HashMap<String, Instant>>,
}

impl MediaSendDedup {
    /// Window within which an identical send is treated as a duplicate. Wide
    /// enough to cover a within-turn repeat or a post-timeout retry, short
    /// enough that intentionally sending the same file again later still lands.
    pub(crate) const TTL: Duration = Duration::from_secs(120);

    /// Signature for a media send: action + chat + file reference + caption.
    /// Same file and caption to the same chat via the same action is a dup.
    pub(crate) fn signature(
        action: &str,
        chat_id: i64,
        reference: &str,
        caption: Option<&str>,
    ) -> String {
        format!("{action}|{chat_id}|{reference}|{}", caption.unwrap_or(""))
    }

    /// Record `signature` and report whether the send is fresh (`true`) or a
    /// duplicate of one claimed within [`TTL`](Self::TTL) (`false`). Expired
    /// entries are pruned on each call so the map cannot grow unbounded.
    pub(crate) fn claim(&self, signature: String, now: Instant) -> bool {
        let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
        seen.retain(|_, t| now.duration_since(*t) < Self::TTL);
        if seen.contains_key(&signature) {
            return false;
        }
        seen.insert(signature, now);
        true
    }
}
