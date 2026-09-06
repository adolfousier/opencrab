//! The MTProto runner must die with whoever holds its guard (#1112 merge
//! follow-up). Before the guard, `connect` spawned the runner and dropped the
//! handle, so stopping the watch loop left the connection open and a restart
//! opened a second pool on the same session file.

use std::time::Duration;

use crate::channels::telegram::userbot::runner::AbortOnDrop;

async fn settle() {
    // Give the runtime a chance to observe the abort.
    tokio::time::sleep(Duration::from_millis(10)).await;
}

#[tokio::test]
async fn dropping_the_guard_aborts_the_runner() {
    let handle = tokio::spawn(std::future::pending::<()>());
    let probe = handle.abort_handle();
    let guard = AbortOnDrop::new(handle);
    settle().await;
    assert!(
        !probe.is_finished(),
        "a pending runner is alive while guarded"
    );

    drop(guard);
    settle().await;
    assert!(
        probe.is_finished(),
        "dropping the guard must abort the runner"
    );
}

#[tokio::test]
async fn aborting_the_owning_task_takes_the_runner_down() {
    // Mirrors the manager: the watch task owns the guard; `handle.abort()` on
    // the watch task drops its future, which drops the guard.
    let runner = tokio::spawn(std::future::pending::<()>());
    let probe = runner.abort_handle();
    let guard = AbortOnDrop::new(runner);
    let watch = tokio::spawn(async move {
        std::future::pending::<()>().await;
        drop(guard);
    });
    settle().await;
    assert!(
        !probe.is_finished(),
        "runner alive while the watch task runs"
    );

    watch.abort();
    settle().await;
    assert!(
        probe.is_finished(),
        "aborting the watch task must abort the runner it owns"
    );
}
