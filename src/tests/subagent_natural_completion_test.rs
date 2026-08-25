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

// #1197: resume and team-create completion paths must HONOR mark_completed's
// no-waiter contract by delivering the result to the parent — spawn already
// did; these two discarded the bool and left parents un-woken.
#[test]
fn resumed_and_team_created_agents_deliver_results_to_parent() {
    let resume_src = include_str!("../brain/tools/subagent/resume.rs");
    let team_src = include_str!("../brain/tools/subagent/team/create.rs");
    // (#1198 DRY): resume and team deliver through the shared manager helper
    assert!(
        resume_src.contains("complete_and_deliver(") && team_src.contains("complete_and_deliver("),
        "resume and team paths must deliver through complete_and_deliver (#1197/#1198)"
    );
    let spawn_src = include_str!("../brain/tools/subagent/spawn.rs");
    assert!(
        spawn_src.contains("push_result("),
        "spawn path must push its result to the parent session (#1197)"
    );
    let manager_src = include_str!("../brain/tools/subagent/manager.rs");
    assert!(
        manager_src.contains("pub fn get_parent_session_id")
            && manager_src.contains("pub fn get_label"),
        "manager must expose delivery-identity getters for non-spawn paths (#1197)"
    );
}
