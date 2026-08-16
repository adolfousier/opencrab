//! The "a key is already stored" sentinel, and the one way to read past it.
//!
//! A secret input is seeded with [`EXISTING_KEY_SENTINEL`] rather than the
//! secret itself, so a stored key is never rendered. The marker then has to
//! mean the same thing to every layer that meets it: the TUI that displays the
//! field, the writer that persists it, and the merge that loads it back.
//!
//! It did not. The literal was spelled out at eight call sites, each doing its
//! own `!= "__EXISTING_KEY__"` equality test, and equality is the wrong test:
//! a field seeded with the marker and then typed into holds
//! `__EXISTING_KEY__<typed>`, which every one of those checks reads as an
//! ordinary credential. That is how a real key reached keys.toml with the
//! marker glued to its front, a value that 401s exactly like a wrong key with
//! nothing on screen to explain it.
//!
//! One place, so every layer agrees by construction. [`real_key`] is the only
//! sanctioned way to ask what a stored value actually carries.

/// Placeholder shown in a secret field when a key is already on disk.
///
/// Never a credential. It exists so the UI can say "a key is set" without
/// reading the secret back out of keys.toml.
pub const EXISTING_KEY_SENTINEL: &str = "__EXISTING_KEY__";

/// The credential a value actually carries, or `None` when it carries none.
///
/// Handles all three shapes a secret field can hold:
///
/// - the marker alone, meaning "unchanged, keep what is on disk" (`None`)
/// - the marker with a key typed after it, meaning the user replaced the key
///   but nothing cleared the seed first (the typed key)
/// - a plain value, pasted or typed (itself, trimmed)
///
/// Trimming matters: a key pasted with a trailing newline is a common shape,
/// and an untrimmed bearer token is rejected by most gateways.
pub fn real_key(value: &str) -> Option<&str> {
    let typed = value
        .strip_prefix(EXISTING_KEY_SENTINEL)
        .unwrap_or(value)
        .trim();
    (!typed.is_empty()).then_some(typed)
}

/// Whether a value carries a usable credential.
///
/// Prefer [`real_key`] where the key itself is wanted: this answers the
/// question but throws away the sanitised value, and a caller that then uses
/// the original string reintroduces the very bug this module exists to close.
pub fn is_real_key(value: &str) -> bool {
    real_key(value).is_some()
}

/// Whether a field still carries the marker, alone or as a prefix.
///
/// Drives display and the clear-on-first-edit behaviour of secret inputs.
pub fn is_stored_marker(value: &str) -> bool {
    value.starts_with(EXISTING_KEY_SENTINEL)
}
