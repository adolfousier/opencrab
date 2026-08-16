//! The stored-key marker means one thing everywhere (#1075).
//!
//! `__EXISTING_KEY__` seeds a secret input so a stored key is never rendered.
//! The literal used to be spelled out at eight call sites, each with its own
//! `!= "__EXISTING_KEY__"` equality test, and equality is the wrong test: a
//! field seeded with the marker and then typed into holds
//! `__EXISTING_KEY__<typed>`, which every one of those checks reads as an
//! ordinary credential.
//!
//! Fixtures are synthetic and carry no real credentials.

use crate::config::stored_key::{EXISTING_KEY_SENTINEL, is_real_key, is_stored_marker, real_key};

#[test]
fn a_plain_key_is_returned_untouched() {
    assert_eq!(real_key("sk-plain"), Some("sk-plain"));
    assert!(is_real_key("sk-plain"));
    assert!(!is_stored_marker("sk-plain"));
}

#[test]
fn the_marker_alone_carries_no_credential() {
    assert_eq!(real_key(EXISTING_KEY_SENTINEL), None);
    assert!(!is_real_key(EXISTING_KEY_SENTINEL));
    assert!(is_stored_marker(EXISTING_KEY_SENTINEL));
}

#[test]
fn a_key_typed_after_the_marker_survives_without_the_marker() {
    // The shape that reached keys.toml: seeded field, pasted into, saved whole.
    // Sanitising rather than rejecting is what lets an already-poisoned
    // keys.toml heal on the next load instead of needing a hand edit.
    let poisoned = format!("{EXISTING_KEY_SENTINEL}sk-typed-after");
    assert_eq!(real_key(&poisoned), Some("sk-typed-after"));
    assert!(is_real_key(&poisoned));
    assert!(is_stored_marker(&poisoned));
}

#[test]
fn whitespace_never_counts_as_a_credential() {
    for blank in ["", "   ", "\n"] {
        assert_eq!(real_key(blank), None, "must not accept {blank:?}");
    }
    assert_eq!(real_key(&format!("{EXISTING_KEY_SENTINEL}  ")), None);
}

#[test]
fn a_pasted_key_is_trimmed() {
    // A key pasted with a trailing newline is a common shape, and an untrimmed
    // bearer token is rejected by most gateways.
    assert_eq!(real_key("  sk-padded \n"), Some("sk-padded"));
    assert_eq!(
        real_key(&format!("{EXISTING_KEY_SENTINEL} sk-padded ")),
        Some("sk-padded")
    );
}

#[test]
fn the_marker_is_only_recognised_as_a_prefix() {
    // A key that merely contains the marker somewhere in the middle is not a
    // seeded field, and stripping there would corrupt the key.
    let odd = "sk-__EXISTING_KEY__-inside";
    assert!(!is_stored_marker(odd));
    assert_eq!(real_key(odd), Some(odd));
}
