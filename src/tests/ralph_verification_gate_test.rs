//! The Ralph loop verification gate (#852).
//!
//! The gate exists because a model can report a task as `success` without the
//! work having passed, and from inside the turn the claim and the truth look
//! identical. It replaces self-report with a shell exit code.
//!
//! None of it was testable: every path ran `sh -c`. `verify_with` now takes the
//! runner as a seam, so the verdict logic can be exercised directly, and
//! `commands_for_type` is pure over the config section.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::brain::tools::plan_tool::{
    RalphVerification, TaskTypeCommands, commands_for_type, truncate_output, verify_with,
};

fn cmds(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// A runner that answers from a table and records what it was asked to run.
fn fake<'a>(
    table: Vec<(&'static str, i32, &'static str)>,
    log: &'a mut Vec<String>,
) -> impl FnMut(&str) -> (i32, String) + 'a {
    move |cmd: &str| {
        log.push(cmd.to_string());
        table
            .iter()
            .find(|(c, _, _)| *c == cmd)
            .map(|(_, code, out)| (*code, out.to_string()))
            .unwrap_or((0, String::new()))
    }
}

// ── The verdict ─────────────────────────────────────────────────────────────

#[test]
fn all_commands_passing_accepts_the_task() {
    let mut log = Vec::new();
    let mut run = fake(vec![("cargo test", 0, "ok")], &mut log);
    assert!(verify_with("build", 1, &cmds(&["cargo test"]), true, &mut run).is_ok());
}

#[test]
fn a_nonzero_exit_rejects_the_task() {
    // The whole point: the model cannot self-report success past this.
    let mut log = Vec::new();
    let mut run = fake(
        vec![("cargo clippy", 101, "error: mismatched types")],
        &mut log,
    );
    let err = verify_with("build", 3, &cmds(&["cargo clippy"]), true, &mut run)
        .expect_err("a failing command must reject");
    assert!(err.contains("REJECTED"), "{err}");
}

#[test]
fn the_rejection_names_the_command_the_code_and_the_task() {
    // A bare "rejected" leaves the model guessing, which is how a gate turns
    // into a retry loop.
    let mut log = Vec::new();
    let mut run = fake(
        vec![("cargo clippy", 101, "error: mismatched types")],
        &mut log,
    );
    let err = verify_with("build", 7, &cmds(&["cargo clippy"]), true, &mut run).unwrap_err();
    assert!(err.contains("cargo clippy"), "missing command: {err}");
    assert!(err.contains("101"), "missing exit code: {err}");
    assert!(err.contains("#7"), "missing task order: {err}");
    assert!(err.contains("mismatched types"), "missing output: {err}");
}

#[test]
fn no_commands_means_nothing_to_verify() {
    let mut log = Vec::new();
    let mut run = fake(vec![], &mut log);
    assert!(verify_with("docs", 1, &[], true, &mut run).is_ok());
}

// ── require_all_pass ────────────────────────────────────────────────────────

#[test]
fn require_all_runs_every_command_and_reports_all_failures() {
    let mut log = Vec::new();
    {
        let mut run = fake(
            vec![("a", 1, "a failed"), ("b", 0, ""), ("c", 2, "c failed")],
            &mut log,
        );
        let err = verify_with("build", 1, &cmds(&["a", "b", "c"]), true, &mut run).unwrap_err();
        assert!(err.contains("a failed"), "first failure missing: {err}");
        assert!(err.contains("c failed"), "later failure missing: {err}");
    }
    assert_eq!(
        log,
        vec!["a", "b", "c"],
        "require_all must not short-circuit"
    );
}

#[test]
fn without_require_all_the_first_failure_short_circuits() {
    // Contract, not optimisation: a cheap check placed first is meant to spare
    // an expensive one after it.
    let mut log = Vec::new();
    {
        let mut run = fake(vec![("fast", 1, "fast failed"), ("slow", 0, "")], &mut log);
        assert!(verify_with("build", 1, &cmds(&["fast", "slow"]), false, &mut run).is_err());
    }
    assert_eq!(log, vec!["fast"], "the remaining commands must not run");
}

#[test]
fn without_require_all_a_pass_still_continues_to_the_next() {
    // Short-circuit is on failure only. Stopping on the first success would
    // let a later failing command through.
    let mut log = Vec::new();
    {
        let mut run = fake(vec![("first", 0, ""), ("second", 1, "boom")], &mut log);
        assert!(verify_with("build", 1, &cmds(&["first", "second"]), false, &mut run).is_err());
    }
    assert_eq!(log, vec!["first", "second"]);
}

#[test]
fn a_runner_error_is_a_failure_not_a_pass() {
    // run_verification_command yields -1 when the process cannot start. That
    // must reject, never sail through as if verified.
    let mut log = Vec::new();
    let mut run = fake(vec![("missing-binary", -1, "Failed to execute")], &mut log);
    assert!(verify_with("build", 1, &cmds(&["missing-binary"]), true, &mut run).is_err());
}

// ── Task-type mapping ───────────────────────────────────────────────────────

fn verification(enabled: bool, pairs: &[(&str, &[&str])]) -> RalphVerification {
    RalphVerification {
        enabled,
        require_all_pass: true,
        task_type_commands: pairs
            .iter()
            .map(|(t, c)| TaskTypeCommands {
                task_type: t.to_string(),
                commands: cmds(c),
            })
            .collect(),
    }
}

#[test]
fn the_matching_task_type_selects_its_commands() {
    let v = verification(
        true,
        &[("build", &["cargo clippy"]), ("test", &["cargo test"])],
    );
    assert_eq!(
        commands_for_type(&v, "build"),
        Some(cmds(&["cargo clippy"]))
    );
    assert_eq!(commands_for_type(&v, "test"), Some(cmds(&["cargo test"])));
}

#[test]
fn task_type_matching_is_case_insensitive() {
    let v = verification(true, &[("build", &["cargo clippy"])]);
    assert_eq!(
        commands_for_type(&v, "BUILD"),
        Some(cmds(&["cargo clippy"]))
    );
}

#[test]
fn an_unmapped_task_type_verifies_nothing() {
    let v = verification(true, &[("build", &["cargo clippy"])]);
    assert_eq!(commands_for_type(&v, "docs"), None);
}

#[test]
fn an_empty_command_list_verifies_nothing() {
    // Otherwise an empty list would report a vacuous pass, which reads as
    // verified when nothing ran.
    let v = verification(true, &[("build", &[])]);
    assert_eq!(commands_for_type(&v, "build"), None);
}

#[test]
fn disabling_verification_turns_the_gate_off_entirely() {
    // Documents the failure mode rather than endorsing it: with the gate off
    // every task passes unverified, and a TOML typo produces this same state
    // silently.
    let v = verification(false, &[("build", &["cargo clippy"])]);
    assert_eq!(commands_for_type(&v, "build"), None);
}

// ── Output truncation ───────────────────────────────────────────────────────

#[test]
fn short_output_is_untouched() {
    assert_eq!(truncate_output("brief", 500), "brief");
}

#[test]
fn long_output_is_truncated_and_marked() {
    let long = "x".repeat(600);
    let out = truncate_output(&long, 500);
    assert!(out.ends_with("...[truncated]"));
    assert!(out.len() < long.len() + 20);
}

#[test]
fn truncation_does_not_split_a_multibyte_character() {
    // The original sliced `&output[..500]`, which panics when byte 500 lands
    // inside a character. Compiler output makes that likely: rustc emits `─`
    // and smart quotes freely, so the gate would panic on the very failure it
    // exists to report.
    for filler in ["─", "é", "🦀", "…"] {
        let noisy = filler.repeat(400);
        let out = truncate_output(&noisy, 500);
        assert!(
            out.ends_with("...[truncated]"),
            "truncation failed for {filler:?}"
        );
    }
}

#[test]
fn truncating_at_an_exact_boundary_is_stable() {
    let exact = "y".repeat(500);
    assert_eq!(
        truncate_output(&exact, 500),
        exact,
        "no marker when it fits"
    );
}
