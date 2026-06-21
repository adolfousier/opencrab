//! Owner-impersonation detection for group chats.
//!
//! The owner is identified by user ID, never by display name. But a non-owner
//! can set their name/username to the owner's to socially engineer the agent.
//! `mimics_owner` flags that so the harness can prepend a warning; these tests
//! pin the normalization and the match/no-match cases.

use crate::channels::telegram::handler::{mimics_owner, normalize_identity};

#[test]
fn normalize_strips_case_space_and_symbols() {
    assert_eq!(normalize_identity("Adolfo Usier"), "adolfousier");
    assert_eq!(normalize_identity("adolfo  usier"), "adolfousier");
    assert_eq!(normalize_identity("AdolfoUsier!"), "adolfousier");
    assert_eq!(normalize_identity("🔺 Adolfo Usier"), "adolfousier");
    assert_eq!(normalize_identity("   "), "");
}

#[test]
fn exact_name_is_flagged() {
    assert!(mimics_owner("Adolfo Usier", None, "Adolfo Usier", None));
}

#[test]
fn case_space_and_symbol_variants_are_flagged() {
    assert!(mimics_owner("adolfo  usier", None, "Adolfo Usier", None));
    assert!(mimics_owner("AdolfoUsier", None, "Adolfo Usier", None));
    assert!(mimics_owner("Adolfo Usier!", None, "Adolfo Usier", None));
}

#[test]
fn matching_username_is_flagged() {
    assert!(mimics_owner(
        "Totally Not The Owner",
        Some("adolfousier"),
        "Adolfo Usier",
        Some("adolfousier"),
    ));
}

#[test]
fn cross_name_vs_username_is_flagged() {
    // Sender's display name equals the owner's @username.
    assert!(mimics_owner(
        "adolfousier",
        None,
        "Adolfo Usier",
        Some("adolfousier"),
    ));
    // Sender's @username equals the owner's display name (normalized).
    assert!(mimics_owner(
        "Someone",
        Some("AdolfoUsier"),
        "Adolfo Usier",
        None,
    ));
}

#[test]
fn genuinely_different_user_is_not_flagged() {
    assert!(!mimics_owner(
        "Carlos Cunha",
        Some("carlos"),
        "Adolfo Usier",
        Some("adolfousier"),
    ));
}

#[test]
fn blank_sender_never_matches_blank_owner() {
    // Empty normalized forms must not collide into a false positive.
    assert!(!mimics_owner("   ", Some("!!!"), "", Some("")));
    assert!(!mimics_owner("", None, "Adolfo Usier", Some("adolfousier")));
}
