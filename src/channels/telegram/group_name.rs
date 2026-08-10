//! Records a Telegram group's human-readable title into its config section.
//!
//! `[channels.telegram.groups.<chat_id>]` is keyed by a numeric chat id, so
//! anyone reading config (a person, or the agent answering a question about
//! which groups exist) sees only numbers (#984). The title rides on every
//! group message; this stores it beside the ACL so the authoritative place
//! carries a name too.
//!
//! The stored value is display metadata. Access control keys off the chat id
//! and never consults it: a group title is chosen by whoever administers the
//! group, so it is untrusted text.

use crate::config::Config;
use crate::config::types::TelegramConfig;

/// Longest title stored. Telegram caps group titles at 128 characters; the
/// bound is re-applied here because the value arrives over the wire.
const MAX_LEN: usize = 128;

/// Normalise an observed chat title for storage.
///
/// Collapses whitespace (titles carry newlines and decorative spacing), drops
/// control characters that would otherwise sit inside a TOML string, and
/// bounds the length. Returns `None` when nothing printable survives, so a
/// group with a blank or hostile title stays unnamed rather than storing junk.
pub(crate) fn sanitize(title: &str) -> Option<String> {
    let printable: String = title
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let collapsed = printable.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return None;
    }
    // Truncation is by character, not byte, so a multi-byte title can never be
    // cut mid-codepoint. The trailing trim tidies a cut that lands on a space.
    let bounded: String = collapsed.chars().take(MAX_LEN).collect();
    let bounded = bounded.trim_end();
    if bounded.is_empty() {
        None
    } else {
        Some(bounded.to_string())
    }
}

/// Does the stored name need rewriting to match what was just observed?
///
/// Groups get renamed, so a stored name that no longer matches is stale and
/// worse than none: it names the wrong room.
pub(crate) fn needs_update(stored: Option<&str>, observed: &str) -> bool {
    stored != Some(observed)
}

/// Persist a configured group's title when it is missing or stale.
///
/// Only touches groups that already have a config section. A group the owner
/// never configured stays out of config entirely rather than accumulating a
/// name-only section for every room the bot is passing through.
///
/// Returns whether a write happened, so callers can log the transition once
/// instead of on every message.
pub fn record(tg: &TelegramConfig, chat_id: &str, title: Option<&str>) -> Result<bool, String> {
    let Some(group) = tg.groups.get(chat_id) else {
        return Ok(false);
    };
    let Some(observed) = title.and_then(sanitize) else {
        return Ok(false);
    };
    if !needs_update(group.name.as_deref(), &observed) {
        return Ok(false);
    }
    // Written as a string explicitly: a group named "2026" would otherwise be
    // type-inferred into an integer and stop deserializing.
    Config::write_key_string(
        &format!("channels.telegram.groups.{chat_id}"),
        "name",
        &observed,
    )
    .map(|()| true)
    .map_err(|e| format!("Failed to write group name for {chat_id}: {e}"))
}
