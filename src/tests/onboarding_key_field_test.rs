//! Sentinel semantics for secret inputs in the onboarding dialogs (#1039).
//!
//! A field seeded with `EXISTING_KEY_SENTINEL` means "a key is stored, not
//! shown". The renderer used to read that as no key while the save path read
//! it as a value worth writing, so a configured provider displayed as empty
//! and its key was overwritten with the literal marker on the next save.

use crate::tui::onboarding::key_field::{is_configured, is_stored, masked, typed_secret};
use crate::tui::provider_selector::EXISTING_KEY_SENTINEL;

fn is_new_secret(value: &str) -> bool {
    typed_secret(value).is_some()
}

#[test]
fn the_sentinel_is_recognised_as_a_stored_key() {
    assert!(is_stored(EXISTING_KEY_SENTINEL));
    assert!(!is_stored("sk-real-key"));
    assert!(!is_stored(""));
}

#[test]
fn typing_into_a_seeded_field_persists_only_what_was_typed() {
    // Nothing clears the marker on the first keystroke, so a pre-filled field
    // that is typed into reads `__EXISTING_KEY__sk-...`. The equality check
    // this replaces called that a fresh secret and stored it verbatim, leaving
    // a key that 401s for a reason nothing on screen explains.
    let concatenated = format!("{EXISTING_KEY_SENTINEL}sk-typed-after-the-marker");
    assert!(is_stored(&concatenated));
    assert_eq!(
        typed_secret(&concatenated),
        Some("sk-typed-after-the-marker")
    );
}

#[test]
fn the_marker_alone_yields_nothing_to_write() {
    assert_eq!(typed_secret(EXISTING_KEY_SENTINEL), None);
    // Whitespace after the marker is not a key either.
    assert_eq!(
        typed_secret(&format!("{EXISTING_KEY_SENTINEL}   ")),
        None,
        "trailing whitespace must not count as a typed key"
    );
}

#[test]
fn a_plain_typed_key_is_returned_untouched() {
    assert_eq!(typed_secret("sk-real-key"), Some("sk-real-key"));
    // A pasted key often carries a trailing newline; an untrimmed bearer token
    // is rejected by most gateways.
    assert_eq!(typed_secret("  sk-padded \n"), Some("sk-padded"));
    assert_eq!(typed_secret(""), None);
}

#[test]
fn a_stored_key_counts_as_configured() {
    // The display bug: this used to be false, so a provider with a key
    // rendered as if it had none.
    assert!(is_configured(EXISTING_KEY_SENTINEL));
}

#[test]
fn a_typed_key_counts_as_configured() {
    assert!(is_configured("sk-real-key"));
}

#[test]
fn an_empty_field_is_not_configured() {
    assert!(!is_configured(""));
}

#[test]
fn the_sentinel_is_never_written() {
    // The destructive bug: an emptiness check let this through and persisted
    // the marker over a working key.
    assert!(!is_new_secret(EXISTING_KEY_SENTINEL));
}

#[test]
fn an_empty_field_is_never_written() {
    assert!(!is_new_secret(""));
}

#[test]
fn a_typed_key_is_written() {
    assert!(is_new_secret("sk-real-key"));
}

#[test]
fn configured_and_writable_disagree_only_on_the_sentinel() {
    // The two gates must differ for exactly one input. Anywhere else they
    // diverge is a place the display and the save path could drift apart
    // again.
    for value in ["", "sk-real-key", "gsk_something", EXISTING_KEY_SENTINEL] {
        let differ = is_configured(value) != is_new_secret(value);
        assert_eq!(
            differ,
            value == EXISTING_KEY_SENTINEL,
            "{value:?} should only differ when it is the sentinel"
        );
    }
}

#[test]
fn a_stored_key_renders_as_saved_rather_than_blank() {
    let shown = masked(EXISTING_KEY_SENTINEL);
    assert!(
        !shown.is_empty(),
        "a stored key must not render as an empty field"
    );
    assert!(shown.contains("saved"));
}

#[test]
fn a_typed_key_is_never_rendered_in_the_clear() {
    let secret = "sk-super-secret-value";
    let shown = masked(secret);
    assert!(!shown.contains(secret));
    assert!(!shown.contains("sk-"));
}

#[test]
fn an_empty_field_renders_empty_so_the_placeholder_shows() {
    assert!(masked("").is_empty());
}

#[test]
fn masking_is_bounded_for_a_very_long_key() {
    // Keeps a pathological paste from overflowing the field.
    let shown = masked(&"x".repeat(500));
    assert!(shown.chars().count() <= 20);
}
