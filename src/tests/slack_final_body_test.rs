//! The final Slack message must not repeat what the step group already shows
//! (#1010).
//!
//! Narration folds into the step group (#943) and ALSO stays in the final
//! `response.content`. The completion path compared hashes of whole messages,
//! so a body containing every note plus the answer matched nothing and shipped
//! entire.
//!
//! The first attempt at this compared whole paragraphs and was a no-op on real
//! data: narration and final body are split at STREAM offsets, so the same
//! sentence is cut mid-word between them and no paragraph on one side equals a
//! paragraph on the other. Matching therefore runs over non-whitespace
//! characters, which is what these tests pin.

use crate::channels::slack::final_body::{folded_paragraphs, strip_folded_notes};

/// The shape that defeated the paragraph version, observed live.
///
/// The group folded a note ending mid-word ("the s") and the final body
/// continued it ("harper"). Nothing is equal on either side except the
/// non-whitespace stream.
#[test]
fn a_note_cut_mid_word_is_still_matched() {
    let folded = folded_paragraphs(Some("You're right, and it's the s".to_string()));
    let body = "You're right, and it's the s\n\nharper version of the point.\n\n\
                I knew the delivery mechanism was a read-only bind mount.";

    let out = strip_folded_notes(body, &folded);
    assert!(
        out.starts_with("harper version of the point."),
        "mid-word split was not consumed: {out:?}"
    );
    assert!(!out.contains("You're right"), "narration survived: {out:?}");
}

/// The reported failure: every folded note, in order, ahead of the answer.
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
                The service is in a restart loop.";

    let out = strip_folded_notes(body, &folded);
    assert_eq!(out, "The service is in a restart loop.");
}

/// Formatting drift between the fold and the final must not defeat the match.
///
/// This is the normalization Telegram already had and Slack lacked.
#[test]
fn whitespace_differences_do_not_defeat_the_match() {
    let folded = folded_paragraphs(Some("Checking   the\nsocket now.".to_string()));
    let body = "Checking the socket now.\n\nThe socket is absent.";
    assert_eq!(strip_folded_notes(body, &folded), "The socket is absent.");
}

/// A turn with no narration comes through byte-identical.
#[test]
fn a_body_with_no_folded_notes_is_untouched() {
    let body = "Done. The socket line is added and the loop has stopped.";
    assert_eq!(strip_folded_notes(body, &[]), body);
    assert_eq!(strip_folded_notes(body, &folded_paragraphs(None)), body);
}

/// A final that is a fresh answer, not a continuation, is left whole.
#[test]
fn a_body_that_does_not_start_with_the_narration_is_untouched() {
    let folded = folded_paragraphs(Some("Checking the scan log.".to_string()));
    let body = "The log holds zero verdicts since boot.";
    assert_eq!(strip_folded_notes(body, &folded), body);
}

/// Consumption stops at the first note that diverges.
///
/// Everything from the divergence onward is the answer and must survive, even
/// though a later note would also have matched.
#[test]
fn consumption_stops_at_the_first_divergence() {
    let folded = vec![
        "First note.".to_string(),
        "A note the final never repeats.".to_string(),
        "Third note.".to_string(),
    ];
    let body = "First note.\n\nThird note.\n\nThe answer.";

    let out = strip_folded_notes(body, &folded);
    assert_eq!(
        out, "Third note.\n\nThe answer.",
        "divergence must halt consumption rather than skip ahead"
    );
}

/// A body that is narration and nothing else yields nothing.
///
/// The caller's empty-final guard then treats the folded narration as the
/// answer, which is the correct outcome: it is already on screen in the group.
#[test]
fn a_body_that_is_entirely_narration_yields_empty() {
    let folded = folded_paragraphs(Some("Checking.\n\nStill checking.".to_string()));
    let body = "Checking.\n\nStill checking.";
    assert!(strip_folded_notes(body, &folded).trim().is_empty());
}

/// Prose the group never folded always survives.
#[test]
fn prose_the_group_never_folded_survives() {
    let folded = folded_paragraphs(Some("Checking the socket now.".to_string()));
    let body = "Checking the socket now.\n\n\
                Let me be clear about what I did not verify: the staging box.";

    let out = strip_folded_notes(body, &folded);
    assert_eq!(
        out,
        "Let me be clear about what I did not verify: the staging box."
    );
}
