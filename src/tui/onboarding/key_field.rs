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
//! - [`typed_secret`]: what did the user actually type, if anything? Drives
//!   writes. The sentinel never survives this, alone or as a prefix.
//! - [`masked`] — what to show in the field, never the secret.

/// True when the field still carries the "a key is already stored" marker.
///
/// Prefix, not equality. Clearing on the first keystroke is best effort and was
/// missing on three voice fields, so a pre-filled field that is typed into can
/// hold `__EXISTING_KEY__<typed>`. An equality test reads that as a freshly
/// typed secret, which is how a real key ended up stored with the marker glued
/// to its front, a value that fails auth exactly like a wrong key with nothing
/// on screen to say why.
pub(crate) fn is_stored(value: &str) -> bool {
    crate::config::stored_key::is_stored_marker(value)
}

/// The secret to persist, or `None` when the field says "leave what is on disk".
///
/// The only value a write may use. Strips a leading marker so the key typed
/// after it is kept rather than dropped, and returns `None` when nothing but
/// the marker (or whitespace) is left.
pub(crate) fn typed_secret(value: &str) -> Option<&str> {
    crate::config::stored_key::real_key(value)
}

/// Drop the seeded marker so an edit replaces the stored key instead of being
/// appended to it. Call before the first mutation of a secret input.
pub(crate) fn clear_marker_before_edit(buf: &mut String) {
    if is_stored(buf) {
        buf.clear();
    }
}

/// True when a key exists for this provider, whether stored earlier or typed
/// just now. Use for display and for "is this configured?" checks. The
/// sentinel counts, because it only ever appears when a key is stored.
pub(crate) fn is_configured(value: &str) -> bool {
    !value.is_empty()
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
