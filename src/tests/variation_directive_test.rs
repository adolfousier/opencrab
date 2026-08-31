//! The shared variation directive (#32).
//!
//! Born from the 2026-08-29 incident: a ship call recurred through the
//! loop-guard ladder (nudge at 3-in-8, break at 4-in-8) and the turn broke
//! silently. The reason the call failed sat in the first tool result — the
//! work had already been done by a sibling lane — but no guard message made
//! the model read it and act on it. The directive is that missing
//! instruction, stated once and shared by every loop-breaker that composes
//! it (bash Layer-3 rejection, tool-loop near-match nudge).
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::brain::agent::service::nudge::{loop_guard_breadcrumb, variation_directive};

#[test]
fn it_forbids_reissuing_the_same_call() {
    // The opening sentence is the stop order. Without it the rest reads as a
    // suggestion and a determined retry walks straight through it.
    let d = variation_directive();
    assert!(d.contains("Do not re-issue the same call"), "{d}");
}

#[test]
fn it_points_at_the_result_already_in_hand() {
    // The load-bearing lesson from the incident: the answer was already in
    // the first tool result. The directive must send the model back to read
    // it, not just tell it to try something else.
    let d = variation_directive();
    assert!(d.contains("result you already have"), "{d}");
    assert!(
        d.contains("already done"),
        "must name the common reason: {d}"
    );
}

#[test]
fn it_requires_a_state_check_before_reacting() {
    // "Verify current state" is what turns "the work is already done" from
    // a guess into a check — the incident model never re-checked.
    let d = variation_directive();
    assert!(d.contains("Verify current state"), "{d}");
}

#[test]
fn it_offers_completion_as_an_exit() {
    // Same escape-hatch logic as FINISHED_ESCAPE: if the state check shows
    // nothing left to do, reporting completion must be a sanctioned action,
    // or the model invents a pointless call to comply.
    let d = variation_directive();
    assert!(d.contains("report completion"), "{d}");
}

#[test]
fn the_break_breadcrumb_names_the_call_count_and_window() {
    // The user must see WHICH call tripped the guard and how hard, or the
    // breadcrumb reads as a generic apology and teaches nothing.
    let b = loop_guard_breadcrumb("bash", 4, 8);
    assert!(b.contains("'bash' recurred 4x in the last 8 steps"), "{b}");
}

#[test]
fn the_break_breadcrumb_says_nothing_is_queued_and_how_to_resume() {
    // The 2026-08-29 incident: the break was silent for half an hour
    // because nothing said the turn had ended and no work was pending.
    // Both facts must be visible — nothing queued, and the word to resume.
    let b = loop_guard_breadcrumb("oc-deploy", 4, 8);
    assert!(b.contains("Nothing is queued"), "{b}");
    assert!(b.contains("say the word to resume"), "{b}");
}
