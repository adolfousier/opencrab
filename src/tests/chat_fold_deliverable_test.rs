//! A report survives the fold even when it is not the final answer (#904).
//!
//! `visible_when_folded` kept exactly one assistant row per turn: the single
//! `final_idx`. A turn that produced a real deliverable and then said anything
//! after it therefore folded the deliverable away and exposed the trailing
//! remark instead. The user saw a collapsed turn and a completion that looked
//! eaten — it was present in the store, just hidden.
//!
//! Pressing Esc on a follow-up suggestion appeared to cause this. It did not;
//! it only revealed it.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::tui::render::chat::is_deliverable_report;

const TABLE_REPORT: &str = "Here is the breakdown you asked for.\n\n\
| Commit | What |\n\
|--------|------|\n\
| aaaaaaa | first change landed on main with tests |\n\
| bbbbbbb | second change landed on main with tests |\n\
| ccccccc | third change landed on main with tests |\n\n\
All green, nothing pushed.";

#[test]
fn a_table_report_qualifies() {
    assert!(is_deliverable_report(TABLE_REPORT));
}

#[test]
fn a_trailing_remark_does_not() {
    // The row that was being exposed INSTEAD of the report.
    assert!(!is_deliverable_report("Done. Anything else?"));
}

#[test]
fn long_narration_without_a_table_does_not_qualify() {
    // Length alone must not promote prose: a long chain of narration is still
    // narration, and promoting it would defeat the fold entirely.
    let narration = "I am going to check the repo state, then stage the files, \
        then commit them, then verify the result once more before reporting back \
        to you with what happened and what remains outstanding after that."
        .repeat(3);
    assert!(narration.chars().count() > 200);
    assert!(!is_deliverable_report(&narration));
}

#[test]
fn a_short_table_does_not_qualify() {
    // A two-cell scrap is not a deliverable; requiring bulk keeps the fold
    // meaningful.
    assert!(!is_deliverable_report("| a |\n|---|\n| b |"));
}

#[test]
fn a_pipe_character_in_prose_is_not_a_table() {
    // Shell pipes and maths appear in ordinary answers and must not promote
    // them.
    let prose = "Run `grep foo | wc -l` to count them, and note that a | b \
        is the union here. "
        .repeat(4);
    assert!(prose.chars().count() > 200);
    assert!(!is_deliverable_report(&prose));
}

#[test]
fn a_separator_row_is_required() {
    // Pipe-delimited lines without a separator are not a GFM table.
    let no_sep = "| one | two |\n| three | four |\n".repeat(10);
    assert!(no_sep.chars().count() > 200);
    assert!(!is_deliverable_report(&no_sep));
}
