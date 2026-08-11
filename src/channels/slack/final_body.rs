//! Remove narration from the final body that the step group already shows
//! (#1010).
//!
//! Since #943 per-step narration folds into the collapsible step group. The
//! final `response.content` carries the same narration, because it is
//! assistant text like any other, and nothing took it back out. The completion
//! path could not catch it: it compares the final body's HASH against one
//! intermediate's, which fires only when the model repeats a single
//! intermediate verbatim. A body that contains every note plus the answer
//! equals none of them, so nothing was suppressed and the whole turn's
//! commentary shipped ahead of the answer.
//!
//! Telegram does not have this problem because it normalizes whitespace before
//! comparing (`delivery.rs`, `norm`). Slack hashed raw bytes, so any formatting
//! drift missed. This module ports that normalization and extends it to the
//! shape Slack actually produces.
//!
//! ## Why matching cannot be done on paragraphs
//!
//! The narration and the final body are split at STREAM offsets, not at
//! paragraph boundaries, so the same sentence can be cut mid-word between them:
//!
//! ```text
//! group note : "You're right, and it's the s"
//! final body : "You're right, and it's the s\n\nharper version of the point."
//! ```
//!
//! Comparing paragraphs, or whole strings, or hashes, all fail here — no unit
//! on either side is equal to a unit on the other. Comparison therefore runs
//! over the sequence of NON-WHITESPACE characters, where the split is invisible
//! because whitespace is exactly what differs.
//!
//! Still structural: the group holds the exact strings it folded, and only
//! those are consumed, in the order they were folded. Nothing is inferred from
//! whether prose reads like narration, which is the constraint #928 and #1009
//! settled on.

/// Non-whitespace characters of `s`, each with its byte offset in `s`.
fn dense(s: &str) -> Vec<(char, usize)> {
    s.char_indices()
        .filter(|(_, c)| !c.is_whitespace())
        .map(|(i, c)| (c, i))
        .collect()
}

/// Drop the narration the step group already displays from the front of
/// `body`.
///
/// Consumes `folded` in order, advancing through `body` for as long as each
/// note matches. Stops at the first note that does not, so a body that
/// diverges partway keeps everything from the divergence onward. Returns
/// `body` unchanged when the first note does not match at all, which is the
/// case where the final is a fresh answer rather than a continuation.
pub(crate) fn strip_folded_notes(body: &str, folded: &[String]) -> String {
    if folded.is_empty() || body.trim().is_empty() {
        return body.to_string();
    }

    let body_dense = dense(body);
    let mut cursor = 0usize; // index into body_dense

    for note in folded {
        let note_dense: Vec<char> = note.chars().filter(|c| !c.is_whitespace()).collect();
        if note_dense.is_empty() {
            continue;
        }
        let end = cursor + note_dense.len();
        if end > body_dense.len() {
            break;
        }
        let matches = body_dense[cursor..end]
            .iter()
            .map(|(c, _)| *c)
            .eq(note_dense.iter().copied());
        if !matches {
            break;
        }
        cursor = end;
    }

    if cursor == 0 {
        return body.to_string();
    }
    match body_dense.get(cursor) {
        // Cut at the next surviving character, so the answer keeps its own
        // leading formatting rather than inheriting the narration's.
        Some((_, byte)) => body[*byte..].to_string(),
        // Every dense character belonged to the narration: the body was
        // narration and nothing else.
        None => String::new(),
    }
}

/// The narration strings a group has folded, in fold order, ready for
/// [`strip_folded_notes`].
pub(crate) fn folded_paragraphs(notes: Option<String>) -> Vec<String> {
    notes
        .into_iter()
        .flat_map(|n| {
            n.split("\n\n")
                .map(|p| p.trim().to_string())
                .collect::<Vec<_>>()
        })
        .filter(|p| !p.is_empty())
        .collect()
}
