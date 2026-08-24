//! Regression pins for #1184 (sub-agent natural completion).
//!
//! Before the fix, all three agent loops (`spawn`, `resume`, `team/create`)
//! parked every round end in `AwaitingInput` regardless of why the round
//! ended. Since v0.3.81 nothing ever delivered fire-and-forget results: the
//! agent finished its work in minutes and then sat as phantom-"Running"
//! forever, because `push_result` only fires on terminal states.
//!
//! The fix: a round whose `stop_reason` is not `ToolUse` means the model
//! finished its answer, so the loop breaks to the completion tail
//! (`mark_completed` -> push). Only genuinely gated rounds (approval prompt,
//! iteration cap) keep the parking behavior.
//!
//! These are source-shape pins (house style): they assert the guard exists
//! in all three loops, so deleting any one of them fails loudly instead of
//! silently resurrecting the phantom-Running bug.

use std::path::Path;

const SITES: [(&str, &str); 3] = [
    ("spawn.rs", "src/brain/tools/subagent/spawn.rs"),
    ("resume.rs", "src/brain/tools/subagent/resume.rs"),
    ("team/create.rs", "src/brain/tools/subagent/team/create.rs"),
];

fn repo_path(rel: &str) -> String {
    // Tests run from the crate root; fall back to the manifest parent for
    // workspace layouts.
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    p.to_string_lossy().into_owned()
}

#[test]
fn every_agent_loop_guards_round_end_with_natural_completion() {
    for (name, rel) in SITES {
        let src = std::fs::read_to_string(repo_path(rel))
            .unwrap_or_else(|e| panic!("cannot read {rel}: {e}"));

        assert!(
            src.contains("Natural completion (#1184)"),
            "{name}: natural-completion guard comment missing - the \
             phantom-Running regression (#1184) may be back"
        );
        assert!(
            src.contains("!= Some(crate::brain::provider::types::StopReason::ToolUse)"),
            "{name}: ToolUse stop-reason gate missing"
        );
    }
}

#[test]
fn parking_is_now_the_exception_not_the_default() {
    // Each loop must still park ONCE (for genuinely gated rounds), but the
    // park can no longer be the unconditional first response to a round end.
    for (name, rel) in SITES {
        let src = std::fs::read_to_string(repo_path(rel))
            .unwrap_or_else(|e| panic!("cannot read {rel}: {e}"));

        let parks = src.matches("mark_awaiting_input(&agent_id_clone)").count();
        assert_eq!(
            parks, 1,
            "{name}: expected exactly 1 gated parking site, found {parks}"
        );

        // The guard must textually precede the park inside the same Ok arm.
        let guard = src
            .find("Natural completion (#1184)")
            .expect("{name}: guard not found");
        let park = src
            .find("mark_awaiting_input(&agent_id_clone)")
            .expect("{name}: park not found");
        assert!(
            guard < park,
            "{name}: natural-completion guard must come before the park call"
        );
    }
}
