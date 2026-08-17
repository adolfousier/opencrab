//! Dropped tool futures must reap their child process (#1046).
//!
//! A timed-out tool drops its future, which drops the `tokio::process::Child`.
//! Tokio does NOT kill the process on drop unless `kill_on_drop(true)` was set,
//! so without it a timed-out `grep` kept running after its turn had settled and
//! reported a final answer produced without it.
//!
//! These pin the semantics the fix relies on. If a tokio upgrade ever changed
//! them, the tools would silently start leaking processes again and nothing
//! else in the suite would notice.

use tokio::process::Command;

/// Is the process still alive? `kill -0` signals nothing and only checks.
fn alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Give the OS a moment to reap, then report whether the pid is gone.
async fn gone_within(pid: u32, tries: u32) -> bool {
    for _ in 0..tries {
        if !alive(pid) {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    !alive(pid)
}

#[tokio::test]
async fn dropping_a_child_with_kill_on_drop_reaps_the_process() {
    let child = Command::new("sleep")
        .arg("30")
        .kill_on_drop(true)
        .spawn()
        .expect("spawn sleep");
    let pid = child.id().expect("child has a pid");
    assert!(alive(pid), "sanity: the process started");

    drop(child);

    assert!(
        gone_within(pid, 30).await,
        "kill_on_drop(true) must reap the child when the future is dropped"
    );
}

#[tokio::test]
async fn a_timed_out_command_does_not_outlive_its_future() {
    // The reported shape: a long command wrapped in a timeout. When the
    // timeout fires the future is dropped, and the process must go with it.
    let mut child = Command::new("sleep")
        .arg("30")
        .kill_on_drop(true)
        .spawn()
        .expect("spawn sleep");
    let pid = child.id().expect("child has a pid");

    let waited = tokio::time::timeout(std::time::Duration::from_millis(200), child.wait()).await;
    assert!(waited.is_err(), "sanity: the command outlives its timeout");

    drop(child);

    assert!(
        gone_within(pid, 30).await,
        "a timed-out command must not keep running after its turn moves on"
    );
}
