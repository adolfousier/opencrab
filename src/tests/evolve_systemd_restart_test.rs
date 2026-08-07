//! Sentinel tests for the `evolve` tool's systemd restart helpers.
//!
//! Context — PR #137 (#136) added a post-swap step that schedules a
//! delayed `systemctl restart opencrabs*.service` via `systemd-run`.
//! Two gaps from the PR review remained open after merge:
//!
//!   #3  Silent failure when no units match the glob — the agent
//!       would say "Evolved!" and the daemon would never restart
//!       (zero matching units), the exact symptom #136 was filed for.
//!   #6  No test coverage on the restart path — flag drift in
//!       systemd-run args could silently break the restart again.
//!
//! These tests pin both: the systemd-run command construction stays
//! stable across refactors, AND the user-facing message for every
//! `RestartStatus` outcome clearly tells the user what actually
//! happened (or didn't).
//!
//! What we DON'T test: actual systemd interaction. Spawning real
//! `systemd-run` requires a Linux host with systemd running, which
//! isn't portable across CI (macOS + Windows have no systemd).
//! Construction + message shape is what we can pin portably; the
//! end-to-end behaviour was validated empirically by the PR author.

use crate::brain::tools::evolve::{
    EVOLVE_UNIT_GLOB, SYSTEMD_UNIT_PATTERN, build_systemd_cleanup_command,
    build_systemd_restart_command,
};

#[test]
fn unit_pattern_is_glob_so_multiple_profiles_match() {
    // Adding a new profile (opencrabs-staging.service) must not
    // require a code change. The pattern is shipped as a public
    // const so refactors that hardcode "opencrabs.service" would
    // diverge from the tested invariant.
    assert_eq!(
        SYSTEMD_UNIT_PATTERN, "opencrabs*.service",
        "the glob must match every opencrabs-*.service variant; a non-glob value \
         would silently break multi-profile restart"
    );
}

#[test]
fn restart_command_uses_systemd_run_binary() {
    let cmd = build_systemd_restart_command(12345, false);
    assert_eq!(
        cmd.get_program(),
        "systemd-run",
        "command must invoke systemd-run, not systemctl directly — only the \
         transient unit escapes the daemon cgroup"
    );
}

#[test]
fn restart_command_system_level_args_are_pinned() {
    let cmd = build_systemd_restart_command(12345, false);
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .collect();
    assert_eq!(
        args,
        vec![
            "--on-active=3",
            "--unit=opencrabs-evolve-12345",
            "systemctl",
            "restart",
            "opencrabs*.service",
        ],
        "system-level (user=false) arg list must not drift — each flag's removal or rename re-introduces \
         a known regression mode: --on-active=3 = the 3s delivery window, \
         --unit=... = the PID-derived name that avoids concurrent-evolve collisions, \
         opencrabs*.service = the multi-profile glob. \
         NOTE: --collect and --quiet are intentionally absent (incompatible with \
         systemd < v240 on RHEL 7 / CentOS 7); do NOT re-add them without \
         confirming the minimum systemd version policy."
    );
}

#[test]
fn restart_command_user_level_includes_user_flag() {
    let cmd = build_systemd_restart_command(12345, true);
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .collect();
    assert_eq!(
        args,
        vec![
            "--user",
            "--on-active=3",
            "--unit=opencrabs-evolve-12345",
            "systemctl",
            "--user",
            "restart",
            "opencrabs*.service",
        ],
        "user-level (user=true) command must include --user on both systemd-run \
         (to connect to the user bus and create the timer in the user instance) \
         and systemctl (to target the user service manager)"
    );
}

#[test]
fn restart_command_unit_name_includes_pid() {
    // Concurrent evolve calls would collide on a fixed transient
    // unit name. The PID embedding makes the name unique per
    // process. Verify both that the PID appears verbatim AND that
    // different PIDs produce different names.
    let cmd_a = build_systemd_restart_command(12345, false);
    let cmd_b = build_systemd_restart_command(67890, false);
    let unit_a = cmd_a
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .find(|a| a.starts_with("--unit="))
        .expect("unit arg must exist");
    let unit_b = cmd_b
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .find(|a| a.starts_with("--unit="))
        .expect("unit arg must exist");
    assert_eq!(unit_a, "--unit=opencrabs-evolve-12345");
    assert_eq!(unit_b, "--unit=opencrabs-evolve-67890");
    assert_ne!(
        unit_a, unit_b,
        "two concurrent evolves on different PIDs must produce different unit names \
         or systemd-run will fail on the second one"
    );
}

// ── stale-unit cleanup (the accumulation fix) ───────────────────

#[test]
fn cleanup_uses_reset_failed_not_collect() {
    // We can't pass --collect to systemd-run (unsupported on systemd
    // < v240, RHEL 7 / CentOS 7), so spent transient units are swept
    // with `systemctl reset-failed <glob>` instead. reset-failed has
    // existed far longer than --collect, so it stays portable.
    let cmd = build_systemd_cleanup_command(false);
    assert_eq!(
        cmd.get_program(),
        "systemctl",
        "cleanup must use systemctl reset-failed, not systemd-run --collect"
    );
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .collect();
    assert_eq!(
        args,
        vec!["reset-failed", "opencrabs-evolve-*.service"],
        "system-level cleanup must reset-failed exactly the evolve-unit glob — \
         a wider glob could clear unrelated units, a narrower one would miss \
         accumulated failed restarts"
    );
}

#[test]
fn cleanup_glob_matches_restart_unit_names() {
    // The cleanup glob must match the per-PID unit names that
    // build_systemd_restart_command creates, or nothing gets swept.
    let restart = build_systemd_restart_command(12345, false);
    let unit = restart
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .find(|a| a.starts_with("--unit="))
        .expect("unit arg must exist");
    assert_eq!(unit, "--unit=opencrabs-evolve-12345");
    // EVOLVE_UNIT_GLOB is "opencrabs-evolve-*.service"; the unit name is
    // "opencrabs-evolve-12345" (systemd appends .service). Assert the
    // glob's fixed prefix matches so the two can never drift apart.
    let prefix = EVOLVE_UNIT_GLOB.trim_end_matches("*.service");
    assert_eq!(prefix, "opencrabs-evolve-");
    assert!(
        "opencrabs-evolve-12345".starts_with(prefix),
        "cleanup glob prefix must match the restart unit-name prefix"
    );
}

#[test]
fn cleanup_user_level_includes_user_flag() {
    let cmd = build_systemd_cleanup_command(true);
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .collect();
    assert_eq!(
        args,
        vec!["--user", "reset-failed", "opencrabs-evolve-*.service"],
        "user-level cleanup must target the user manager with --user, matching \
         the user-level restart path"
    );
}

#[test]
fn evolve_sweeps_stale_units_before_scheduling_restart() {
    // Sentinel: the schedule path must call the cleanup before spawning
    // the restart. Removing it re-introduces the accumulation bug where
    // failed opencrabs-evolve-* units pile up forever.
    let src = include_str!("../brain/tools/evolve/mod.rs");
    assert!(
        src.contains("build_systemd_cleanup_command(use_user_units)"),
        "evolve must sweep stale evolve units (reset-failed) before scheduling a \
         new restart, or failed transient units accumulate without bound"
    );
}

// ── User-message coverage for every RestartStatus branch ────────

#[test]
fn restart_status_messages_are_distinct_per_outcome() {
    let src = include_str!("../brain/tools/evolve/mod.rs");
    assert!(
        src.contains("Evolved from v{current} to v{latest}."),
        "the Scheduled branch must confirm the evolve completed"
    );
    assert!(
        src.contains("Binary updated on disk; restart"),
        "the NotSystemd branch must tell the user the binary is updated but they need to restart"
    );
    assert!(
        src.contains("no \\\n                 systemd units matched")
            || src.contains("no systemd units matched"),
        "the NoUnitsMatched branch must explicitly call out the zero-units case — \
         silently saying 'Restarting…' here is the #136 regression"
    );
    assert!(
        src.contains("scheduling the systemd restart failed"),
        "the SpawnFailed branch must quote the actual error so the user knows \
         systemd-run couldn't fire"
    );
}

#[test]
fn no_units_matched_message_mentions_user_flag() {
    // The user-facing message should guide the user toward
    // `systemctl --user restart` as well as the system variant.
    let src = include_str!("../brain/tools/evolve/mod.rs");
    assert!(
        src.contains("systemctl --user restart"),
        "NoUnitsMatched user message must mention --user restart as an option"
    );
}

#[test]
fn spawn_failed_message_mentions_user_flag() {
    let src = include_str!("../brain/tools/evolve/mod.rs");
    assert!(
        src.contains("systemctl --user restart"),
        "SpawnFailed user message must mention --user restart as an option"
    );
}

#[test]
fn evolve_falls_back_to_user_level_when_system_level_empty() {
    // The core fix in PR #162: when system-level count returns 0,
    // evolve must check user-level units before giving up.
    // This sentinel ensures the fallback logic doesn't get removed.
    let src = include_str!("../brain/tools/evolve/mod.rs");
    assert!(
        src.contains("count_matching_systemd_units(SYSTEMD_UNIT_PATTERN, true)"),
        "evolve must fall back to user-level unit count when system-level returns 0, \
             removing this re-introduces the 'evolve said success but daemon didn't restart' bug (#136)"
    );
}

#[test]
fn evolve_logs_user_level_unit_count_on_fallback() {
    // When the fallback triggers, evolve must log the user-level count
    // so operators can debug "why didn't my daemon restart" from logs.
    let src = include_str!("../brain/tools/evolve/mod.rs");
    assert!(
        src.contains("using {n} user-level units"),
        "evolve must log user-level unit count on fallback for debugging, \
         silent fallbacks make #136-style issues impossible to diagnose"
    );
}
