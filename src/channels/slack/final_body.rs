//! Remove narration from the final body that the step group already shows
//! (#1010).
//!
//! Since #943, per-step narration folds into the collapsible step group rather
//! than posting standalone. The final `response.content`, however, still
//! carries that same narration, because it is assistant text like any other and
//! nothing ever took it back out.
//!
//! The completion path only ever deleted standalone intermediate POSTS whose
//! hash matched the final. Folded narration is in neither place: it is not a
//! standalone post, and the final body equals no single intermediate — it
//! contains all of them. So nothing suppressed it and the whole turn's
//! commentary shipped ahead of the answer, ten "let me check X" lines deep on a
//! ten-step turn.
//!
//! The strip is structural, never a guess about which prose looks like
//! narration: the group holds the exact strings it folded, so only text already
//! visible in the group is removed. Anything the group never saw is untouched,
//! which is the same constraint #928 and #1009 settled on.

/// Drop paragraphs from `body` that the step group already displays.
///
/// Compares whole paragraphs by trimmed equality. A narration line that the
/// model rewrote before repeating it is left alone: a near-match is not proof,
/// and dropping it would be the classification this deliberately avoids.
///
/// Returns the body unchanged when `folded` is empty, so a turn with no
/// narration cannot be altered by this path.
pub(crate) fn strip_folded_notes(body: &str, folded: &[String]) -> String {
    if folded.is_empty() {
        return body.to_string();
    }
    let folded: Vec<&str> = folded
        .iter()
        .map(|f| f.trim())
        .filter(|f| !f.is_empty())
        .collect();
    if folded.is_empty() {
        return body.to_string();
    }

    let kept: Vec<&str> = body
        .split("\n\n")
        .filter(|para| {
            let t = para.trim();
            !t.is_empty() && !folded.contains(&t)
        })
        .collect();

    kept.join("\n\n")
}

/// The narration strings a group has folded, ready for [`strip_folded_notes`].
///
/// Splits each note on blank lines so a multi-paragraph note still matches the
/// body paragraph-for-paragraph.
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
