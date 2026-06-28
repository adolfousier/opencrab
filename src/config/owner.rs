//! Shared bot-owner identity resolution.
//!
//! Every channel config (Telegram, Discord, Slack, WhatsApp, Trello) exposes a
//! `bot_owner` allow list and an `is_owner()` method. To keep the semantics
//! identical across channels — and to make the rules unit-testable in one
//! place — the actual resolution lives here.
//!
//! Two modes govern ownership:
//!
//! - **Open mode**: when the channel's allow list is empty, the bot responds to
//!   everyone, so everyone is treated as an owner. This mirrors the allowlist
//!   gate ("empty = accept all") and avoids locking owners out of an
//!   unconfigured channel.
//! - **Positional fallback**: when `bot_owner` is left unset, the owner defaults
//!   to the *first* entry in the allow list. This is the user who set the
//!   channel up. Setting `bot_owner` explicitly overrides this and is the
//!   recommended, unambiguous configuration.

/// Decide whether `user_id` is a bot owner for a channel.
///
/// * If `allowed` is empty, the channel is in **open mode** and everyone is an
///   owner (returns `true`).
/// * Otherwise, if `bot_owner` is non-empty, ownership is exactly membership in
///   `bot_owner`.
/// * Otherwise (no explicit owners configured), the owner is the **first** entry
///   in `allowed` — the positional fallback to the setup user.
pub fn is_owner(allowed: &[String], bot_owner: &[String], user_id: &str) -> bool {
    if allowed.is_empty() {
        return true;
    }
    if !bot_owner.is_empty() {
        bot_owner.iter().any(|o| o == user_id)
    } else {
        allowed.first().is_some_and(|o| o == user_id)
    }
}

/// Compute an explicit `bot_owner` list seeded from the allow list, or `None`
/// when no seeding is needed/possible.
///
/// * If `bot_owner` is already non-empty, ownership is explicit; returns `None`
///   (nothing to seed).
/// * Otherwise the owner is seeded from the **first** allow-list entry (the
///   setup user), returning `Some(vec![first])`.
/// * If `allowed` is also empty there is nobody to seed from; returns `None`.
pub fn seed_bot_owner(allowed: &[String], bot_owner: &[String]) -> Option<Vec<String>> {
    if !bot_owner.is_empty() {
        return None;
    }
    allowed.first().map(|o| vec![o.clone()])
}
