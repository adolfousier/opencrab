//! #1192: an announcement introduced by an em dash, colon or semicolon must
//! still be offered to the ^-anchored work-announcement regex.
//!
//! The splitter behind `matches_work_announcement` used to break only on
//! `.!?\n…`, so a gerund sitting after a clause introducer was never tested
//! and a zero-tool turn shipped as a finished answer.

use crate::brain::agent::service::phantom::{
    has_phantom_tool_intent_no_tools, matches_work_announcement,
};

/// The reported turn, minus the fenced block that followed it.
const REPORTED_LEAD: &str = "Queue locked: everything except #933/#1112. \
     That's ten issues — fetching all specs now:";

#[test]
fn test_em_dash_introduced_announcement_is_detected() {
    assert!(
        matches_work_announcement(REPORTED_LEAD),
        "#1192: gerund after an em dash was never offered to the regex"
    );
}

#[test]
fn test_clause_introducers_all_open_a_candidate_start() {
    for lead in [
        "That's ten issues — fetching all specs now:",
        "Two things left: running the tests now.",
        "Plan is set; pushing the branch now.",
        "Ten issues – checking each one now.",
    ] {
        assert!(
            matches_work_announcement(lead),
            "#1192: clause introducer did not open a start: {lead:?}"
        );
    }
}

#[test]
fn test_reported_turn_is_phantom_end_to_end() {
    // What the zero-tool gate actually receives: prose, blank line, fence.
    let turn = format!("{REPORTED_LEAD}\n\n```bash\ngh issue view 1176\n```");
    assert!(
        has_phantom_tool_intent_no_tools(&turn),
        "#1192: the reported zero-tool turn still reads as a real answer"
    );
}

#[test]
fn test_prose_after_a_colon_is_not_an_announcement() {
    // The widening must stay bounded by the regex's marker requirement: no
    // trailing " now", ellipsis, or terminal colon means no announcement,
    // however many candidate starts the splitter now produces.
    for lead in [
        "Here is what I found: reading the file is straightforward.",
        "Three options: checking the cache, or a full rebuild, are both fine.",
        "The docs say: running it now takes about a minute.",
        "Two notes; testing was already green before this change.",
    ] {
        assert!(
            !matches_work_announcement(lead),
            "#1192: false positive on ordinary prose: {lead:?}"
        );
    }
}

#[test]
fn test_previously_matching_forms_still_match() {
    // Additive change: nothing that matched before may stop matching.
    for lead in [
        "Running checks now.",
        "Internet's back, pushing now.",
        "Apologies. Pushing now.",
        "Ok, checking the logs…",
    ] {
        assert!(
            matches_work_announcement(lead),
            "#1192 regression: {lead:?} stopped matching"
        );
    }
}
