//! One instance per profile, claimed before any startup work (#1072).
//!
//! A second daemon for the same profile used to boot all the way through: two
//! full memory reindexes over one memory.db, two provider factories, two sets
//! of channel connect attempts. The scheduler lock and the channel token locks
//! caught it later and only partially, by which point the duplicate work was
//! already done and the process stayed up serving nothing.
//!
//! Every test points at a TempDir. The real `~/.opencrabs/locks/` is off limits
//! here: `preempt_instances_in` SIGTERMs the PIDs it finds under it, and a test
//! that reached the real directory would kill the user's running instance.

use crate::config::profile::{InstanceGuard, acquire_instance_lock_in};

fn held_pid(guard: &InstanceGuard) -> Option<Option<u32>> {
    match guard {
        InstanceGuard::Held { pid } => Some(*pid),
        _ => None,
    }
}

#[test]
fn the_first_claim_wins_and_the_second_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let first = acquire_instance_lock_in(dir.path(), "default");
    assert!(
        matches!(first, InstanceGuard::Acquired(_)),
        "a free profile must be claimable"
    );

    let second = acquire_instance_lock_in(dir.path(), "default");
    assert!(
        matches!(second, InstanceGuard::Held { .. }),
        "a second instance of the same profile must be refused, not queued"
    );
}

#[test]
fn the_holder_pid_is_reported_so_the_user_can_find_it() {
    // "Already running" with no PID is a dead end for whoever reads it. The
    // stamp is what turns the refusal into an actionable message.
    let dir = tempfile::tempdir().unwrap();
    let _held = acquire_instance_lock_in(dir.path(), "default");
    let denied = acquire_instance_lock_in(dir.path(), "default");
    assert_eq!(
        held_pid(&denied),
        Some(Some(std::process::id())),
        "the refusal must name the live process that owns the profile"
    );
}

#[test]
fn releasing_the_lock_frees_the_profile() {
    // The kernel drops an flock when the fd closes, on a clean exit or a crash,
    // so a killed instance must never wedge a profile.
    let dir = tempfile::tempdir().unwrap();
    {
        let first = acquire_instance_lock_in(dir.path(), "default");
        assert!(matches!(first, InstanceGuard::Acquired(_)));
    }
    let after = acquire_instance_lock_in(dir.path(), "default");
    assert!(
        matches!(after, InstanceGuard::Acquired(_)),
        "the profile must be claimable again once the holder is gone"
    );
}

#[test]
fn profiles_do_not_block_each_other() {
    // The whole point of profiles. A daemon on `default` and a TUI on `hermes`
    // are two legitimate instances, and a guard that conflated them would be
    // worse than no guard.
    let dir = tempfile::tempdir().unwrap();
    let default = acquire_instance_lock_in(dir.path(), "default");
    let hermes = acquire_instance_lock_in(dir.path(), "hermes");
    assert!(matches!(default, InstanceGuard::Acquired(_)));
    assert!(matches!(hermes, InstanceGuard::Acquired(_)));
}

#[test]
fn a_stale_pid_stamp_is_not_reported_as_the_owner() {
    // A lock file can outlive the process that stamped it. Reporting a dead PID
    // would send the user hunting a process that does not exist; the flock, not
    // the stamp, is what actually decides.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("default.lock"), "4294967294").unwrap();
    let guard = acquire_instance_lock_in(dir.path(), "default");
    assert!(
        matches!(guard, InstanceGuard::Acquired(_)),
        "an unlocked file with a dead PID stamp must not block a claim"
    );
}

#[test]
fn an_unusable_lock_dir_does_not_stop_the_boot() {
    // An unwritable lock dir is an environment problem. Refusing to start over
    // it would turn a warning into a machine the user cannot boot, which is a
    // worse failure than the duplicate work the guard prevents.
    let dir = tempfile::tempdir().unwrap();
    let file_in_the_way = dir.path().join("not-a-dir");
    std::fs::write(&file_in_the_way, b"x").unwrap();
    let guard = acquire_instance_lock_in(&file_in_the_way, "default");
    assert!(
        matches!(guard, InstanceGuard::Unavailable),
        "an unusable lock dir must degrade to unguarded, never to refused"
    );
}
