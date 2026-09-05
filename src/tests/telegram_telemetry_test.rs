//! Send-correlation telemetry for the Telegram surface (#1085).

use crate::channels::telegram::telemetry::*;

#[test]
fn hash8_is_stable_and_8_hex_chars() {
    let a = content_hash8("hello world");
    let b = content_hash8("hello world");
    assert_eq!(a, b);
    assert_eq!(a.len(), 8);
    assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn hash8_separates_different_content() {
    assert_ne!(content_hash8("hello world"), content_hash8("hello worlD"));
}

#[test]
fn hash8_handles_empty_and_multibyte() {
    assert_eq!(content_hash8("").len(), 8);
    // PT-PT and emoji content must not panic (multibyte boundary safety).
    assert_eq!(content_hash8("ção 🦀 açúcar").len(), 8);
}
