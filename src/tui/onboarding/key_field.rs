//! Semantics for a secret input pre-filled with [`EXISTING_KEY_SENTINEL`].
//!
//! When a dialog opens on a provider that already has a stored key, the input
//! is seeded with the sentinel rather than the secret itself, so the key is
//! never rendered. That marker has to mean the same thing to everything that
//! looks at the field, and it did not: the renderer read it as "no key here"
//! while the save path read it as a value worth writing. The result was a
//! stored key that displayed as absent and was then overwritten with the
//! literal marker on the next save (#1039).
//!
//! One module, so both halves agree by construction:
//!
//! - [`is_configured`] — is a key set at all, stored or freshly typed? Drives
//!   display.
//! - [`is_new_secret`] — did the user actually type something to persist?
//!   Drives writes. The sentinel never survives this.
//! - [`masked`] — what to show in the field, never the secret.

use super::types::EXISTING_KEY_SENTINEL;

/// True when the field holds the "a key is already stored" marker rather than
/// something the user typed.
pub(crate) fn is_stored(value: &str) -> bool {
    value == EXISTING_KEY_SENTINEL
}

/// True when a key exists for this provider, whether stored earlier or typed
/// just now. Use for display and for "is this configured?" checks. The
/// sentinel counts, because it only ever appears when a key is stored.
pub(crate) fn is_configured(value: &str) -> bool {
    !value.is_empty()
}

/// True when the field carries a secret worth persisting.
///
/// The only gate that may guard a write. An empty field means "not set" and
/// the sentinel means "unchanged, keep what is on disk"; writing either one
/// destroys a working key.
pub(crate) fn is_new_secret(value: &str) -> bool {
    !value.is_empty() && !is_stored(value)
}

/// What to render in the field. Never the secret: a stored key reads as saved,
/// a typed one as a fixed run of dots whose length says nothing about the key.
pub(crate) fn masked(value: &str) -> String {
    if value.is_empty() {
        String::new()
    } else if is_stored(value) {
        "•••••••••• (saved)".to_string()
    } else {
        "•".repeat(value.len().min(20))
    }
}
