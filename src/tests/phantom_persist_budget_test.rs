//! #1172 — phantom detector must not destroy finished work, and its budget
//! must bound wall-clock across provider rotation.
//!
//! Production incident 2026-08-23: a verbose review sub-agent produced its
//! complete final report four times; every copy was classified phantom and
//! dropped ("Skipping DB persist"), then rotation reset the retry budget so
//! the loop regenerated the same report 25 times across 8 provider swaps
//! (123 stream requests, $0 metered) before a manual kill.
//!
//! Two fixes, both inside `run_tool_loop_inner`. Like
//! `thinking_loop_fallback_test`, these pin control-flow properties of the
//! 7k-line loop on its source — reproducing them live needs a provider that
//! narrates on demand.
//!
//! 1. Blocked iterations persist under an explicit `<!-- phantom_blocked=1 -->`
//!    HTML-comment flag (invisible when rendered, greppable in the DB),
//!    superseding #458's turn-close flush.
//! 2. A never-resetting detection ceiling caps a turn at
//!    MAX_PHANTOM_RETRIES × (MAX_PHANTOM_ROLLS + 1) detections total,
//!    independent of swaps; the give-up path fires when it trips, and rolls
//!    cannot rescue a spent ceiling.

const TOOL_LOOP: &str = "src/brain/agent/service/tool_loop.rs";

fn tool_loop_src() -> String {
    std::fs::read_to_string(TOOL_LOOP).expect("tool_loop.rs must be readable")
}

/// Blocked iterations are persisted with the flag, not skipped.
#[test]
fn blocked_iterations_persist_under_the_phantom_flag() {
    let src = tool_loop_src();
    let branch = src
        .find("if iteration_is_phantom {")
        .expect("the phantom persist branch must exist");
    let tail = &src[branch..branch + 3000];
    assert!(
        tail.contains("phantom_blocked=1"),
        "blocked iterations must carry the phantom_blocked=1 marker so a \
         classified-as-phantom deliverable stays recoverable from the DB"
    );
    assert!(
        tail.contains("append_content"),
        "the flagged iteration must actually be written to the message store"
    );
    assert!(
        !tail.contains("pending_phantom_content"),
        "the stash-and-flush mechanism (#458) is superseded; nothing may be \
         withheld any more"
    );
}

/// The user-facing accumulation is untouched by the flagged persist.
#[test]
fn flagged_persist_does_not_reach_the_user_surface() {
    let src = tool_loop_src();
    let branch = src
        .find("if iteration_is_phantom {")
        .expect("the phantom persist branch must exist");
    // Bound at the arm's closing `} else {` — beyond it lies the legitimate
    // non-phantom accumulation, which MUST keep its push_str.
    let else_pos = src[branch..]
        .find("} else {")
        .expect("phantom arm must be followed by an else")
        + branch;
    let arm = &src[branch..else_pos];
    assert!(
        !arm.contains("accumulated_text.push_str"),
        "a phantom-blocked iteration must never enter accumulated_text — \
         the user still sees none of it"
    );
}

/// Both detection sites feed the never-resetting counter.
#[test]
fn both_detection_sites_count_toward_the_ceiling() {
    let src = tool_loop_src();
    let hits = src.matches("phantom_detections_total += 1;").count();
    assert_eq!(
        hits, 2,
        "exactly two increment sites expected: the ThinkingLoopTimeout arm \
         and the main phantom-detection arm"
    );
}

/// Both retry gates stop once the ceiling is spent.
#[test]
fn retry_gates_check_the_global_ceiling() {
    let src = tool_loop_src();
    let hits = src
        .matches("&& phantom_detections_total < MAX_PHANTOM_DETECTIONS_TOTAL")
        .count();
    assert!(
        hits >= 3,
        "the ceiling gate must appear in the timeout guard, the main retry \
         gate and the roll guard; found {hits}"
    );
}

/// A mid-fresh-budget ceiling trip still ends the turn.
#[test]
fn give_up_fires_when_the_ceiling_trips() {
    let src = tool_loop_src();
    assert!(
        src.contains("|| phantom_detections_total >= MAX_PHANTOM_DETECTIONS_TOTAL"),
        "the give-up gate must accept the global ceiling as an exhaustion \
         condition, or a swapped-in provider's fresh budget keeps the loop alive"
    );
}

/// Rolling cannot rescue a spent ceiling.
#[test]
fn rolls_cannot_rescue_a_spent_ceiling() {
    let src = tool_loop_src();
    let roll = src
        .find("if phantom_rolls < MAX_PHANTOM_ROLLS")
        .expect("the roll guard must exist");
    let tail = &src[roll..roll + 400];
    assert!(
        tail.contains("phantom_detections_total < MAX_PHANTOM_DETECTIONS_TOTAL"),
        "a roll resets the retry counter and re-nudges — allowed past a spent \
         ceiling, it would defeat the budget entirely"
    );
}

/// The ceiling is derived from the documented formula (5 × (2+1) = 15).
#[test]
fn ceiling_is_derived_from_the_documented_formula() {
    let src = tool_loop_src();
    assert!(
        src.contains("MAX_PHANTOM_DETECTIONS_TOTAL: u32")
            && src.contains("MAX_PHANTOM_RETRIES * (MAX_PHANTOM_ROLLS + 1)"),
        "the ceiling must stay tied to the per-provider budget constants so the \
         two stay coherent"
    );
}

/// The stash declaration and close-flush are fully gone.
#[test]
fn the_turn_close_flush_is_retired() {
    let src = tool_loop_src();
    assert!(
        !src.contains("pending_phantom_content"),
        "#458's stash was superseded by detection-time persistence; leftover \
         references mean the removal was incomplete"
    );
}
