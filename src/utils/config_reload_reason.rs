//! Tell a transient read race apart from a real error in the config file.
//!
//! Config writes are not atomic, so a watcher that fires between truncate and
//! write reads zero bytes. That is harmless and self-heals, and telling the
//! user to hunt for a typo would send them after one that does not exist.
//!
//! The original test for it was `reason.contains("line 1, column 1")`, on the
//! reasoning that only an empty file reports there. It does not: serde reports
//! a FIELD error against the whole struct, which is also line 1, column 1. So
//! `duplicate field \`a2a\`` was announced as a write race with the assurance
//! "your file is almost certainly fine", while the file had a real error and
//! every edit silently did nothing (#1116).
//!
//! Being wrong in that direction is worse than being unsure: a real error
//! described as transient is actively misleading, whereas a transient one
//! described plainly still resolves itself.

/// Signatures that a parse failure is about the file's CONTENT, not about
/// having read it at a bad moment. Any of these means a human has to act.
const CONTENT_ERRORS: &[&str] = &[
    "duplicate field",
    "unknown field",
    "invalid type",
    "missing field",
    "invalid value",
    // Deliberately NOT a bare "expected": it is a substring of "unexpected
    // eof", which is the empty-read case this function exists to recognise.
    // The specific signatures above already cover the real errors, and an
    // unrecognised error defaults to real regardless.
];

/// Is this failure a transient read race rather than a real config error?
///
/// Requires positive evidence of emptiness and the absence of any content
/// error. Anything unrecognised is treated as real, so an unfamiliar error is
/// reported honestly instead of being waved away.
pub(crate) fn is_transient_read_race(reason: &str, file_is_empty_now: bool) -> bool {
    let r = reason.to_lowercase();
    if CONTENT_ERRORS.iter().any(|sig| r.contains(sig)) {
        return false;
    }
    // An empty read is the only thing this branch legitimately covers.
    file_is_empty_now || r.contains("unexpected eof") || r.contains("empty")
}
