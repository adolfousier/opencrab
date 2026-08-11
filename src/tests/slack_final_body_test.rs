//! The final Slack message must not repeat what the step group already shows
//! (#1010).
//!
//! Since #943 narration folds into the collapsible step group. The final
//! `response.content` still carried it, and the completion path only reconciled
//! standalone intermediate POSTS by hash — folded narration is neither a
//! standalone post nor equal to the final body, so nothing removed it. A
//! ten-step turn delivered ten "let me check X" lines ahead of the answer, in
//! call order, as one message.

use crate::channels::slack::final_body::{folded_paragraphs, strip_folded_notes};

/// The reported failure: every note the group folded, prepended to the answer.
#[test]
fn narration_the_group_already_shows_is_dropped() {
    let folded = folded_paragraphs(Some(
        "Let me verify each claim against the host.\n\n\
         Two discrepancies already. Let me dig before drawing conclusions.\n\n\
         Root cause confirmed. Let me measure the blast radius."
            .to_string(),
    ));
    assert_eq!(folded.len(), 3);

    let body = "Let me verify each claim against the host.\n\n\
                Two discrepancies already. Let me dig before drawing conclusions.\n\n\
                Root cause confirmed. Let me measure the blast radius.\n\n\
                The service is in a restart loop. The config is missing a socket \
                declaration, so the entrypoint waits for a file that never appears.";

    let out = strip_folded_notes(body, &folded);
    assert!(!out.contains("Let me verify"), "narration survived: {out}");
    assert!(!out.contains("Let me dig"), "narration survived: {out}");
    assert!(!out.contains("Let me measure"), "narration survived: {out}");
    assert!(out.starts_with("The service is in a restart loop"));
}

/// A turn with no narration must come through byte-identical.
#[test]
fn a_body_with_no_folded_notes_is_untouched() {
    let body = "Done. The socket line is added and the loop has stopped.";
    assert_eq!(strip_folded_notes(body, &[]), body);
    assert_eq!(strip_folded_notes(body, &folded_paragraphs(None)), body);
}

/// Only text the group actually folded is removed, never a lookalike.
///
/// The strip is structural. Prose that merely reads like narration but was
/// never folded is the model's answer and must survive.
#[test]
fn prose_the_group_never_folded_survives() {
    let folded = folded_paragraphs(Some("Checking the socket now.".to_string()));
    let body = "Checking the socket now.\n\n\
                Let me be clear about what I did not verify: the staging box.";

    let out = strip_folded_notes(body, &folded);
    assert!(!out.contains("Checking the socket now."));
    assert!(
        out.contains("Let me be clear about what I did not verify"),
        "unfolded prose was dropped: {out}"
    );
}

/// A rewritten note is not a match, and is deliberately left alone.
#[test]
fn a_reworded_note_is_not_stripped() {
    let folded = folded_paragraphs(Some("Let me check the scan log.".to_string()));
    let body = "Let me check the scan log now.\n\nThe log holds zero verdicts.";

    let out = strip_folded_notes(body, &folded);
    assert!(
        out.contains("Let me check the scan log now."),
        "a near-match was stripped, which is the classification this avoids: {out}"
    );
}

/// Whitespace differences around a paragraph must not defeat the match.
#[test]
fn surrounding_whitespace_does_not_defeat_the_match() {
    let folded = folded_paragraphs(Some("  Verifying the mount.  ".to_string()));
    let body = "Verifying the mount.\n\nThe mount is read-only.";
    assert_eq!(strip_folded_notes(body, &folded), "The mount is read-only.");
}

/// A folded note repeated verbatim inside the answer is removed once per
/// paragraph, leaving no blank gap behind.
#[test]
fn stripping_leaves_no_empty_paragraph_behind() {
    let folded = folded_paragraphs(Some("Checking.".to_string()));
    let body = "Checking.\n\nFirst finding.\n\nChecking.\n\nSecond finding.";
    assert_eq!(
        strip_folded_notes(body, &folded),
        "First finding.\n\nSecond finding."
    );
}
