//! Tests for Layer 3 of the bash retry-loop guard
//! (`check_recent_failure` / `record_bash_outcome`).
//!
//! Background: Layers 1 (interactive command rejection) and 2 (tool
//! description hints) catch most loops on attempt 1. Layer 3 is the
//! catch-all for any failure mode — if the agent runs the exact same
//! command twice in a row and the first attempt failed, the second
//! attempt is short-circuited with a "stop retrying" message that
//! quotes the prior error.
//!
//! State is process-wide (a `OnceLock<RwLock<HashMap<...>>>`), so each
//! test uses a fresh `Uuid::new_v4()` for isolation — cargo test runs
//! tests in parallel, and an explicit "clear all state" reset would
//! wipe sibling tests' buffers.

// Per-session buffers are keyed on `Uuid`, so each test that uses a
// fresh `Uuid::new_v4()` is naturally isolated from siblings even
// though the underlying state map is process-wide. No explicit reset
// needed — and trying to reset wipes parallel tests' buffers.
use crate::brain::tools::bash::{RECENT_BASH_WINDOW, check_recent_failure, record_bash_outcome};
use uuid::Uuid;

#[test]
fn fresh_session_has_no_history() {
    let sid = Uuid::new_v4();
    assert!(check_recent_failure(sid, "ls -la").is_none());
}

#[test]
fn successful_run_does_not_block_retry() {
    // Successful commands aren't held against a re-run — the agent is
    // free to run `git status` 50 times.
    let sid = Uuid::new_v4();
    record_bash_outcome(sid, "git status".to_string(), false, None);
    assert!(check_recent_failure(sid, "git status").is_none());
}

#[test]
fn failed_run_blocks_exact_retry() {
    // The core case: agent ran a command, it failed, agent tries the
    // exact same string. The second attempt is intercepted with a
    // message that quotes the prior error.
    let sid = Uuid::new_v4();
    record_bash_outcome(
        sid,
        "git push origin main".to_string(),
        true,
        Some("rejected: non-fast-forward".to_string()),
    );
    let msg = check_recent_failure(sid, "git push origin main").expect("should block");
    assert!(msg.contains("already ran this exact command"));
    assert!(msg.contains("Previous error:"));
    assert!(msg.contains("non-fast-forward"));
}

#[test]
fn whitespace_normalization_catches_cosmetic_retries() {
    // The agent sometimes adds trailing whitespace or pads with spaces
    // when "retrying". Trimmed comparison still catches it.
    let sid = Uuid::new_v4();
    record_bash_outcome(
        sid,
        "git pull".to_string(),
        true,
        Some("conflict".to_string()),
    );
    assert!(check_recent_failure(sid, "  git pull  ").is_some());
}

#[test]
fn different_command_is_not_blocked() {
    // A different command (even if related) is a different attempt and
    // should be allowed through.
    let sid = Uuid::new_v4();
    record_bash_outcome(sid, "git push origin main".to_string(), true, None);
    assert!(check_recent_failure(sid, "git push origin main --force-with-lease").is_none());
    assert!(check_recent_failure(sid, "git push origin foo").is_none());
}

#[test]
fn move_to_front_keeps_one_entry_per_unique_command() {
    // Re-recording the same command moves it to the front (latest
    // outcome wins) instead of stacking duplicate entries that would
    // crowd out other history.
    let sid = Uuid::new_v4();
    record_bash_outcome(sid, "cmd_a".to_string(), true, Some("err1".to_string()));
    record_bash_outcome(sid, "cmd_b".to_string(), false, None);
    record_bash_outcome(sid, "cmd_a".to_string(), true, Some("err2".to_string()));

    // The latest outcome's snippet wins.
    let msg = check_recent_failure(sid, "cmd_a").expect("should block");
    assert!(msg.contains("err2"));
    assert!(!msg.contains("err1"));
}

#[test]
fn outcome_can_flip_from_fail_to_success() {
    // If a previously-failed command later succeeds, the entry flips
    // and a subsequent retry is no longer blocked.
    let sid = Uuid::new_v4();
    record_bash_outcome(sid, "make build".to_string(), true, Some("err".to_string()));
    assert!(check_recent_failure(sid, "make build").is_some());

    record_bash_outcome(sid, "make build".to_string(), false, None);
    assert!(check_recent_failure(sid, "make build").is_none());
}

#[test]
fn old_failure_falls_off_window() {
    // After RECENT_BASH_WINDOW newer commands, an old failure should
    // no longer block — the agent has clearly moved on and a deliberate
    // retry-much-later is fine.
    let sid = Uuid::new_v4();
    record_bash_outcome(
        sid,
        "old_failing_cmd".to_string(),
        true,
        Some("err".to_string()),
    );
    for i in 0..RECENT_BASH_WINDOW {
        record_bash_outcome(sid, format!("filler_cmd_{}", i), false, None);
    }
    assert!(check_recent_failure(sid, "old_failing_cmd").is_none());
}

#[test]
fn sessions_are_isolated() {
    // Two sessions running the same failing command must not see each
    // other's history.
    let sid_a = Uuid::new_v4();
    let sid_b = Uuid::new_v4();
    record_bash_outcome(
        sid_a,
        "broken_cmd".to_string(),
        true,
        Some("err".to_string()),
    );
    assert!(check_recent_failure(sid_a, "broken_cmd").is_some());
    assert!(check_recent_failure(sid_b, "broken_cmd").is_none());
}

#[test]
fn rejection_message_names_the_window_size() {
    // The agent should see the size of the look-back window so it
    // knows roughly how to "escape" (different command, or wait it
    // out with other work).
    let sid = Uuid::new_v4();
    record_bash_outcome(sid, "x".to_string(), true, None);
    let msg = check_recent_failure(sid, "x").expect("should block");
    assert!(msg.contains(&RECENT_BASH_WINDOW.to_string()));
}

#[test]
fn rejection_message_carries_the_shared_variation_directive() {
    // #32: the Layer-3 rejection must teach the same lesson every
    // loop-breaker teaches — read the result in hand, verify state,
    // vary or report completion. The 2026-08-29 incident was exactly
    // this fixture: a ship call retried after a sibling lane had
    // already shipped, the reason sitting in the first result.
    let sid = Uuid::new_v4();
    record_bash_outcome(
        sid,
        "oc-deploy ship --sha abc123".to_string(),
        true,
        Some("already shipped by sibling lane".to_string()),
    );
    let msg = check_recent_failure(sid, "oc-deploy ship --sha abc123").expect("should block");
    assert!(msg.contains("Do not re-issue the same call"), "{msg}");
    assert!(msg.contains("result you already have"), "{msg}");
    assert!(msg.contains("report completion"), "{msg}");
    // The frame and the quoted error survive the rewording.
    assert!(msg.contains("already ran this exact command"), "{msg}");
    assert!(msg.contains("Previous error:"), "{msg}");
}
