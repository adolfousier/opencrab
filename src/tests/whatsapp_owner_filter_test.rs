//! WhatsApp owner / self-chat authorization (`wa_should_respond`).
//!
//! The bot pairs AS the owner's account, so the owner talks to it in the
//! "Message Yourself" self-chat (is_from_me, chat == sender). That path must be
//! number-agnostic so a config/paired-number mismatch can never lock the owner
//! out. Allow-listed contacts may also message in; everything else is dropped.

use crate::channels::whatsapp::handler::wa_should_respond;

#[test]
fn empty_allow_list_is_open_mode() {
    // No allow list configured -> respond to everyone.
    assert!(wa_should_respond(false, "15550001111", "15550002222", &[]));
    assert!(wa_should_respond(true, "15550001111", "15550001111", &[]));
}

#[test]
fn owner_self_chat_always_authorized() {
    // is_from_me && sender == chat -> the paired owner messaging itself.
    // Number-agnostic: the allow list lists a DIFFERENT number, yet the owner
    // is still authorized in their own self-chat.
    let allowed = vec!["19998887777".to_string()];
    assert!(wa_should_respond(
        true,
        "15550001111",
        "15550001111",
        &allowed
    ));
}

#[test]
fn owner_messaging_a_contact_is_not_self_chat() {
    // is_from_me but sender != chat -> the owner is messaging some OTHER chat
    // (e.g. a contact); not the bot self-chat, so do not respond.
    let allowed = vec!["15550001111".to_string()];
    assert!(!wa_should_respond(
        true,
        "15550001111",
        "15550009999",
        &allowed
    ));
}

#[test]
fn allow_listed_contact_authorized() {
    // !is_from_me && sender in allow list -> respond.
    let allowed = vec!["15550001111".to_string(), "15550003333".to_string()];
    assert!(wa_should_respond(
        false,
        "15550003333",
        "15550003333",
        &allowed
    ));
}

#[test]
fn random_contact_dropped() {
    // !is_from_me && sender NOT in allow list -> ignore.
    let allowed = vec!["15550001111".to_string()];
    assert!(!wa_should_respond(
        false,
        "15559990000",
        "15559990000",
        &allowed
    ));
}

#[test]
fn plus_prefixed_config_entry_matches_bare_sender() {
    // Config stores "+1555...", sender JID yields bare "1555..." — normalize
    // both by stripping the leading '+' before comparing.
    let allowed = vec!["+15550003333".to_string()];
    assert!(wa_should_respond(
        false,
        "15550003333",
        "15550003333",
        &allowed
    ));
}

#[test]
fn plus_prefixed_sender_matches_bare_config_entry() {
    // Symmetric: sender carries a '+', config entry is bare.
    let allowed = vec!["15550003333".to_string()];
    assert!(wa_should_respond(
        false,
        "+15550003333",
        "+15550003333",
        &allowed
    ));
}
