//! TUI-priority preemption: when the interactive TUI starts while a
//! background instance of the same profile already holds the channel
//! token locks (e.g. an `opencrabs daemon` auto-started by systemd on
//! boot), the TUI must shut that instance down so it can own the
//! channels itself. Otherwise `acquire_token_lock` is denied and the
//! user silently gets no Telegram — the "I had to reconnect Telegram"
//! symptom.
//!
//! These tests touch the real `~/.opencrabs/locks` dir and spawn/kill a
//! throwaway child process, so they're `#[ignore]`d (opt in with
//! `cargo test -- --ignored`) and Unix-only — the SIGTERM/SIGKILL path
//! is what we're proving.

#![cfg(unix)]

use crate::config::profile::{
    active_profile, base_opencrabs_dir, hash_token, preempt_other_profile_instances,
    release_token_lock,
};
use std::fs;

/// Serialize against the other lock tests — they all mutate the shared
/// `~/.opencrabs/locks` dir.
fn fs_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[test]
fn no_foreign_instance_means_no_preemption() {
    let _guard = fs_lock();
    // With no foreign lock written for our throwaway channel, preemption
    // must not invent one. (Other real locks in the dir belong to live
    // instances we won't touch in this assertion — we only assert our own
    // channel is absent from the result.)
    let channel = "_test_iso_preempt_none";
    let token_hash = hash_token("preempt-none-token");
    release_token_lock(channel, &token_hash);

    let preempted = preempt_other_profile_instances();
    assert!(
        preempted
            .iter()
            .all(|p| !p.channels.iter().any(|c| c == channel)),
        "no instance should be reported for a channel with no lock file"
    );
}

#[test]
#[ignore = "spawns + kills a child process and touches ~/.opencrabs/locks — run with `cargo test -- --ignored`"]
fn preempts_and_kills_a_live_background_instance() {
    let _guard = fs_lock();
    let base = base_opencrabs_dir();
    let locks_dir = base.join("locks");
    fs::create_dir_all(&locks_dir).unwrap();

    let channel = "_test_iso_preempt_live";
    let token_hash = hash_token("preempt-live-token");
    let lock_path = locks_dir.join(format!("{}_{}.lock", channel, token_hash));

    // Stand in for a background daemon: a child that would otherwise live
    // far longer than this test.
    let mut child = std::process::Command::new("sleep")
        .arg("60")
        .spawn()
        .expect("spawn sleep child");
    let child_pid = child.id();

    // Pin a same-profile lock to that child PID — exactly what a running
    // daemon would have written for a channel credential.
    let current_profile = active_profile().unwrap_or("default");
    fs::write(&lock_path, format!("{}:{}", current_profile, child_pid)).unwrap();

    let preempted = preempt_other_profile_instances();

    let ours = preempted
        .iter()
        .find(|p| p.pid == child_pid)
        .expect("the live background instance must be detected and preempted");
    assert!(
        ours.channels.iter().any(|c| c == channel),
        "the preempted instance must report the channel it held: {:?}",
        ours.channels
    );
    assert!(
        ours.stopped,
        "the background instance must be stopped (SIGTERM, escalating to SIGKILL)"
    );

    // The child must actually be dead. reap it either way so we don't leak.
    let killed = !crate::config::profile::is_pid_alive(child_pid);
    let _ = child.kill();
    let _ = child.wait();
    let _ = fs::remove_file(&lock_path);

    assert!(
        killed,
        "the background process must be gone after preemption"
    );
}
