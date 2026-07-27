//! What counts as a report worth its own message (#824).
//!
//! The rule was `contains_table && len >= 200`, which defines substance as
//! "contains a pipe table". A long prose answer, or one built from headings
//! and code blocks, was therefore classified as narration and folded into the
//! collapsed processing log where the user never reads it.
//!
//! The opposite error matters as much: if ordinary progress chatter starts
//! landing as its own message, the chat fills with "let me check that" and the
//! final answer stops standing out. So both directions are asserted.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::channels::telegram::intermediates::is_substantial_report;

fn prose(n: usize) -> String {
    "The migration completed and the counts reconciled cleanly. ".repeat(n)
}

#[test]
fn a_table_still_qualifies() {
    // The original rule's one correct case must keep working.
    let text = format!("Results:\n\n| a | b |\n|---|---|\n| 1 | 2 |\n{}", prose(4));
    assert!(is_substantial_report(&text, true));
}

#[test]
fn narration_still_folds() {
    // The whole point of folding: passing remarks belong in the log.
    assert!(!is_substantial_report(
        "Let me check the database now.",
        false
    ));
    assert!(!is_substantial_report("Running the query.", false));
}

#[test]
fn a_long_prose_report_no_longer_folds() {
    // The reported gap: substance without a pipe table.
    let text = prose(20);
    assert!(text.chars().count() > 800);
    assert!(
        is_substantial_report(&text, false),
        "a long answer is substance whatever markup it used"
    );
}

#[test]
fn headings_make_a_shorter_answer_a_report() {
    let text = format!("## Findings\n\n{}", prose(5));
    assert!(is_substantial_report(&text, false));
}

#[test]
fn a_fenced_block_makes_it_a_report() {
    // Raw command output is exactly what a user asks to see.
    let text = format!("Output:\n\n```\ntf_true 95\ntotal 1262\n```\n{}", prose(3));
    assert!(is_substantial_report(&text, false));
}

#[test]
fn several_bullets_make_it_a_report() {
    let text = format!(
        "Status:\n- floor NULL 509\n- floors_count NULL 437\n- top_floor NULL 1167\n{}",
        prose(3)
    );
    assert!(is_substantial_report(&text, false));
}

#[test]
fn one_or_two_dashes_are_not_a_list() {
    // A sentence with a dash must not be promoted to a report.
    let text = format!("Two things stood out:\n- the first\n{}", prose(2));
    assert!(!is_substantial_report(&text, false));
}

#[test]
fn a_numbered_list_counts_but_a_leading_digit_does_not() {
    let listed = format!("Steps:\n1. run it\n2. check it\n3. report it\n{}", prose(4));
    assert!(is_substantial_report(&listed, false));

    // "509 rows were affected..." starts with a digit and is not a list.
    let sentence = format!("509 rows were affected by the write. {}", prose(3));
    assert!(!is_substantial_report(&sentence, false));
}

#[test]
fn short_stays_short_however_structured() {
    // Below the floor, even a heading is just chatter.
    assert!(!is_substantial_report("## Done", false));
}
