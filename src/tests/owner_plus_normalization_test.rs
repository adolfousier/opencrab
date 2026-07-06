//! Regression for #277: `is_owner` compared raw strings, so a leading plus
//! in `allowed_phones` / `bot_owner` (natural when pasting international
//! numbers) broke owner gating — the digits extracted from a WhatsApp JID
//! never carry one. Both sides are normalized now.

use crate::config::owner::is_owner;

#[test]
fn plus_in_bot_owner_matches_bare_digits() {
    let allowed = vec!["+15551234567".to_string()];
    let owners = vec!["+15551234567".to_string()];
    assert!(is_owner(&allowed, &owners, "15551234567"));
}

#[test]
fn bare_bot_owner_matches_plus_prefixed_user() {
    let allowed = vec!["15551234567".to_string()];
    let owners = vec!["15551234567".to_string()];
    assert!(is_owner(&allowed, &owners, "+15551234567"));
}

#[test]
fn plus_in_positional_fallback_matches() {
    // No explicit bot_owner: first allow-list entry is the owner.
    let allowed = vec!["+15551234567".to_string(), "15550009999".to_string()];
    assert!(is_owner(&allowed, &[], "15551234567"));
    // Second entry is NOT the owner regardless of prefix.
    assert!(!is_owner(&allowed, &[], "15550009999"));
}

#[test]
fn non_owner_still_rejected() {
    let allowed = vec!["+15551234567".to_string()];
    let owners = vec!["+15551234567".to_string()];
    assert!(!is_owner(&allowed, &owners, "15550000000"));
}

#[test]
fn empty_config_denies_access() {
    // Empty allowed + empty bot_owner = unconfigured = deny all (secure by default).
    assert!(!is_owner(&[], &[], "anyone"));
}
