//! TUI-priority preemption: when the interactive TUI starts while a
//! background instance of the same profile already holds the channel token
//! locks (e.g. an `opencrabs daemon` auto-started by systemd on boot), the
//! TUI shuts that instance down so it can own the channels itself.
//!
//! SAFETY: the preemption sends real SIGTERM/SIGKILL to every live PID it
//! finds in the lock directory. A test must therefore NEVER run it against
//! the real `~/.opencrabs/locks/` — doing so kills the user's own running
//! instances (regression: a `cargo test` run killed the live TUI). These
//! tests exercise the internal `preempt_instances_in`, which takes an explicit
//! lock dir, against a TempDir only, with system-service stopping disabled.

#![cfg(unix)]

use crate::config::profile::{active_profile, is_pid_alive, preempt_instances_in};
use std::fs;
use tempfile::TempDir;

#[test]
fn empty_lock_dir_preempts_nothing() {
    // No lock files → nothing to preempt, and crucially no signals are sent.
    let tmp = TempDir::new().unwrap();
    let preempted = preempt_instances_in(tmp.path(), false);
    assert!(
        preempted.is_empty(),
        "an empty lock dir must preempt nothing, got: {preempted:?}"
    );
}

#[test]
fn dead_pid_lock_is_skipped() {
    // A stale lock owned by a dead PID must not be reported or signalled.
    let tmp = TempDir::new().unwrap();
    let profile = active_profile().unwrap_or("default");
    // PID 999999 is exceedingly unlikely to be alive.
    fs::write(
        tmp.path().join("telegram_deadbeef.lock"),
        format!("{profile}:999999"),
    )
    .unwrap();
    let preempted = preempt_instances_in(tmp.path(), false);
    assert!(
        preempted.is_empty(),
        "a dead-PID lock must be skipped, got: {preempted:?}"
    );
}

#[test]
fn other_profile_lock_is_not_touched() {
    // A live PID under a DIFFERENT profile must never be preempted — even our
    // own (alive) PID, as long as the profile name does not match.
    let tmp = TempDir::new().unwrap();
    let other = format!("not-{}", active_profile().unwrap_or("default"));
    fs::write(
        tmp.path().join("telegram_cafe.lock"),
        format!("{other}:{}", std::process::id()),
    )
    .unwrap();
    let preempted = preempt_instances_in(tmp.path(), false);
    assert!(
        preempted.is_empty(),
        "a different profile's lock must not be preempted, got: {preempted:?}"
    );
}

#[test]
#[ignore = "spawns and kills a child process; run with `cargo test -- --ignored`"]
fn preempts_and_kills_a_live_instance() {
    // Self-contained: write a same-profile lock in a TempDir pointing at a
    // throwaway child we spawned, then assert preemption stops exactly that
    // child. Touches no real lock files and signals no real instances.
    let tmp = TempDir::new().unwrap();
    let profile = active_profile().unwrap_or("default");

    let mut child = std::process::Command::new("sleep")
        .arg("60")
        .spawn()
        .expect("spawn sleep child");
    let child_pid = child.id();

    fs::write(
        tmp.path().join("_test_iso_telegram_abc123.lock"),
        format!("{profile}:{child_pid}"),
    )
    .unwrap();

    let preempted = preempt_instances_in(tmp.path(), false);

    let ours = preempted
        .iter()
        .find(|p| p.pid == child_pid)
        .expect("the live instance must be detected and preempted");
    assert!(
        ours.channels.iter().any(|c| c == "_test_iso_telegram"),
        "preempted instance must report the channel it held: {:?}",
        ours.channels
    );
    assert!(
        ours.stopped,
        "the instance must be stopped (SIGTERM/SIGKILL)"
    );

    let killed = !is_pid_alive(child_pid);
    let _ = child.kill();
    let _ = child.wait();
    assert!(
        killed,
        "the background process must be gone after preemption"
    );
}
