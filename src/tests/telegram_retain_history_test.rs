//! Which chats are worth keeping history for (#1043).
//!
//! Passive capture exists so the bot holds context in a chat it belongs to. It
//! used to run for every undirected message regardless of authorisation, so a
//! group nobody approved still had its members' messages and media written to
//! the database and disk. "Not addressed to us" and "not ours at all" are
//! different things, and only the first should retain anything.

use crate::config::types::{TelegramConfig, TelegramGroupConfig};
use std::collections::HashMap;

fn cfg(allowed: &[&str], owner: &[&str], groups: &[&str]) -> TelegramConfig {
    let mut g = HashMap::new();
    for id in groups {
        g.insert((*id).to_string(), TelegramGroupConfig::default());
    }
    TelegramConfig {
        allowed_users: allowed.iter().map(|s| s.to_string()).collect(),
        bot_owner: owner.iter().map(|s| s.to_string()).collect(),
        groups: g,
        ..Default::default()
    }
}

#[test]
fn a_configured_group_retains_history() {
    // What the owner adding the bot creates. Unchanged behaviour.
    let c = cfg(&["111"], &["111"], &["-100known"]);
    assert!(c.retains_history("-100known", "999"));
}

#[test]
fn an_unknown_chat_retains_nothing() {
    // The incident: added by a stranger, no group entry ever created, and
    // every member's traffic was stored anyway.
    let c = cfg(&["111"], &["111"], &["-100known"]);
    assert!(!c.retains_history("-100stranger", "999"));
}

#[test]
fn an_allowlisted_sender_is_kept_even_in_an_unknown_chat() {
    // Their message is ours to keep wherever they send it, including a chat
    // that has no entry yet.
    let c = cfg(&["111", "222"], &["111"], &[]);
    assert!(c.retains_history("-100stranger", "222"));
}

#[test]
fn the_owner_is_kept_even_in_an_unknown_chat() {
    let c = cfg(&["111"], &["111"], &[]);
    assert!(c.retains_history("-100stranger", "111"));
}

#[test]
fn a_stranger_in_an_unknown_chat_is_not_kept() {
    let c = cfg(&["111"], &["111"], &[]);
    assert!(!c.retains_history("-100stranger", "6784322243"));
}

#[test]
fn a_group_entry_covers_every_sender_in_it() {
    // Membership of the chat is what authorises retention, not who spoke.
    let c = cfg(&["111"], &["111"], &["-100known"]);
    for sender in ["222", "333", "444"] {
        assert!(
            c.retains_history("-100known", sender),
            "{sender} is in an authorised chat"
        );
    }
}

#[test]
fn an_unconfigured_channel_retains_nothing() {
    // With neither allowed_users nor bot_owner set, `config::owner::is_owner`
    // denies everyone, so the bot answers no one. Retaining their messages
    // anyway would be the exact mismatch this issue is about: refusing to
    // talk to someone while still recording them.
    let c = cfg(&[], &[], &[]);
    assert!(!c.retains_history("-100anything", "999"));
}

#[test]
fn a_plus_prefixed_allowlist_entry_still_matches() {
    // The allowlist accepts a leading '+'; retention must read it the same
    // way the access checks do, or the two disagree on the same user.
    let c = cfg(&["+222"], &["111"], &[]);
    assert!(c.retains_history("-100stranger", "222"));
}
