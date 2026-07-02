//! WhatsApp response policy + owner/operator authorization (`wa_should_respond`).
//!
//! The bot pairs AS the paired account, so that account talks to it in the
//! "Message Yourself" self-chat (is_from_me, chat == sender) — always allowed.
//! A `bot_owner` (operator) number DMing the bot is always allowed too. Beyond
//! that, the response policy decides: `auto` (legacy: open when no allow-list,
//! else allow-listed only), `owner_only`, `allowlist`, or `open`.

use crate::channels::whatsapp::handler::wa_should_respond;
use crate::config::types::WaResponsePolicy::{Allowlist, Auto, Open, OwnerOnly};

// Convenience: legacy behaviour is policy = Auto with no operators.
fn legacy(is_from_me: bool, sender: &str, chat: &str, allowed: &[String]) -> bool {
    wa_should_respond(Auto, is_from_me, sender, None, chat, allowed, &[])
}

#[test]
fn auto_empty_allow_list_is_open_mode() {
    assert!(legacy(false, "15550001111", "15550002222", &[]));
    assert!(legacy(true, "15550001111", "15550001111", &[]));
}

#[test]
fn owner_self_chat_always_authorized() {
    // is_from_me && sender == chat -> the paired account messaging itself.
    // Number-agnostic even under a strict policy.
    let allowed = vec!["19998887777".to_string()];
    for p in [Auto, OwnerOnly, Allowlist, Open] {
        assert!(
            wa_should_respond(p, true, "15550001111", None, "15550001111", &allowed, &[]),
            "self-chat must be allowed under {p:?}"
        );
    }
}

#[test]
fn operator_always_authorized() {
    // A bot_owner (operator) number DMing the bot is allowed under every policy,
    // even when not in allowed_phones.
    let operators = vec!["15550001111".to_string()];
    for p in [Auto, OwnerOnly, Allowlist, Open] {
        assert!(
            wa_should_respond(
                p,
                false,
                "15550001111",
                None,
                "15550001111",
                &[],
                &operators
            ),
            "operator must be allowed under {p:?}"
        );
    }
}

#[test]
fn auto_owner_messaging_a_contact_is_not_self_chat() {
    let allowed = vec!["15550001111".to_string()];
    assert!(!legacy(true, "15550001111", "15550009999", &allowed));
}

#[test]
fn auto_allow_listed_contact_authorized() {
    let allowed = vec!["15550001111".to_string(), "15550003333".to_string()];
    assert!(legacy(false, "15550003333", "15550003333", &allowed));
}

#[test]
fn auto_random_contact_dropped() {
    let allowed = vec!["15550001111".to_string()];
    assert!(!legacy(false, "15559990000", "15559990000", &allowed));
}

#[test]
fn owner_only_drops_everyone_else() {
    // OwnerOnly: a contact even on the allow list is refused; only self/operator.
    let allowed = vec!["15550003333".to_string()];
    assert!(!wa_should_respond(
        OwnerOnly,
        false,
        "15550003333",
        None,
        "15550003333",
        &allowed,
        &[]
    ));
}

#[test]
fn allowlist_serves_only_listed_contacts() {
    let allowed = vec!["15550003333".to_string()];
    assert!(wa_should_respond(
        Allowlist,
        false,
        "15550003333",
        None,
        "15550003333",
        &allowed,
        &[]
    ));
    assert!(!wa_should_respond(
        Allowlist,
        false,
        "15559990000",
        None,
        "15559990000",
        &allowed,
        &[]
    ));
}

#[test]
fn open_serves_every_incoming_dm() {
    // Open: a random customer with no allow-list entry is served.
    assert!(wa_should_respond(
        Open,
        false,
        "15559990000",
        None,
        "15559990000",
        &[],
        &[]
    ));
}

#[test]
fn plus_prefix_normalized_both_sides() {
    let allowed = vec!["+15550003333".to_string()];
    assert!(legacy(false, "15550003333", "15550003333", &allowed));
    let allowed_bare = vec!["15550003333".to_string()];
    assert!(legacy(false, "+15550003333", "+15550003333", &allowed_bare));
}

// ── LID-addressed senders (#276) ─────────────────────────────────────────
// WhatsApp addresses most DMs by LID (the privacy id), whose digits never
// equal a phone number. The allow/operator lists hold phone numbers, so a
// LID sender must match through its PN twin (`sender_alt`).

#[test]
fn lid_sender_matches_allowlist_via_pn_alt() {
    let allowed = vec!["15550003333".to_string()];
    assert!(wa_should_respond(
        Auto,
        false,
        "98765432101234", // LID digits
        Some("15550003333"),
        "98765432101234",
        &allowed,
        &[]
    ));
}

#[test]
fn lid_sender_without_alt_is_dropped() {
    let allowed = vec!["15550003333".to_string()];
    assert!(!wa_should_respond(
        Auto,
        false,
        "98765432101234",
        None,
        "98765432101234",
        &allowed,
        &[]
    ));
}

#[test]
fn lid_sender_with_unlisted_alt_is_dropped() {
    let allowed = vec!["15550003333".to_string()];
    assert!(!wa_should_respond(
        Allowlist,
        false,
        "98765432101234",
        Some("15559990000"),
        "98765432101234",
        &allowed,
        &[]
    ));
}

#[test]
fn operator_recognized_via_pn_alt() {
    let operators = vec!["15550001111".to_string()];
    for p in [Auto, OwnerOnly, Allowlist, Open] {
        assert!(
            wa_should_respond(
                p,
                false,
                "98765432101234",
                Some("15550001111"),
                "98765432101234",
                &[],
                &operators
            ),
            "operator via alt must be allowed under {p:?}"
        );
    }
}

#[test]
fn alt_plus_prefix_normalized() {
    let allowed = vec!["+15550003333".to_string()];
    assert!(wa_should_respond(
        Allowlist,
        false,
        "98765432101234",
        Some("+15550003333"),
        "98765432101234",
        &allowed,
        &[]
    ));
}
