//! Tests for the ConfigWatcher callback plumbing.
//!
//! Previously lived inline in `utils/config_watcher.rs` as
//! `#[cfg(test)] mod tests`. Moved out per the project convention that
//! all tests live under `src/tests/`.
//!
//! The `fires_on_change` test builds its own mini-watcher (instead of
//! calling the production `spawn()`) because `spawn()` requires real
//! Config/keys TOML files in the user's home dir — the test just needs
//! to verify that a filesystem change triggers the callback plumbing.

use crate::utils::config_watcher::ReloadCallback;
use notify::Watcher;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn reload_callback_type_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ReloadCallback>();
}

#[tokio::test]
async fn reload_callback_fires_on_change() {
    let tmp = tempfile::tempdir().unwrap();
    let config_path = tmp.path().join("config.toml");
    let keys_path = tmp.path().join("keys.toml");

    std::fs::write(&config_path, "[channels.telegram]\nenabled = false\n").unwrap();
    std::fs::write(&keys_path, "").unwrap();

    let call_count = Arc::new(AtomicUsize::new(0));
    let counter = call_count.clone();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(4);

    let cb: ReloadCallback = Arc::new(move |_cfg| {
        counter.fetch_add(1, Ordering::Relaxed);
        let _ = tx.try_send(());
    });

    // Spawn the watcher and discard its JoinHandle. spawn_blocking tasks are
    // not cancelled by dropping the handle, so the watcher keeps running while
    // the test below mutates the files and waits for the callback. drop() is
    // the explicit, lint-clean way to discard the returned future.
    drop({
        let config_path = config_path.clone();
        let keys_path = keys_path.clone();
        let callbacks = vec![cb];
        tokio::task::spawn_blocking(move || {
            let rt = tokio::runtime::Handle::current();
            let (tx, rx) = std::sync::mpsc::channel();
            let mut watcher = notify::recommended_watcher(move |res| {
                if let Ok(event) = res {
                    let _ = tx.send(event);
                }
            })
            .unwrap();
            let _ = watcher.watch(&config_path, notify::RecursiveMode::NonRecursive);
            let _ = watcher.watch(&keys_path, notify::RecursiveMode::NonRecursive);
            let debounce = std::time::Duration::from_millis(100);
            // Hard deadline so the blocking thread exits and doesn't
            // hang tokio runtime shutdown. Widened from 8s → 20s so
            // the test itself has room to retry writes if FSEvents on
            // macOS coalesces or drops an early event under heavy
            // parallel-test CPU load.
            let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
            loop {
                let remaining = end.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() {
                    break;
                }
                let poll = remaining.min(std::time::Duration::from_millis(200));
                match rx.recv_timeout(poll) {
                    Ok(_) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                }
                let deadline = std::time::Instant::now() + debounce;
                loop {
                    let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                    if remaining.is_zero() {
                        break;
                    }
                    match rx.recv_timeout(remaining) {
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
                for cb in &callbacks {
                    let cb = cb.clone();
                    rt.spawn(async move { cb(crate::config::Config::default()) });
                }
            }
        })
    });

    // Give the watcher thread time to register its subscription before
    // the first write. FSEvents on macOS needs a moment after watch()
    // to actually wire up notifications.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Write a few times with spacing. FSEvents under heavy parallel
    // test load can coalesce or drop rapid events — retrying 3 times
    // with a 400ms gap between writes gives the watcher more than one
    // chance to fire the callback. Each write flips the value so the
    // file mtime changes meaningfully.
    for i in 0..3 {
        let val = if i % 2 == 0 { "true" } else { "false" };
        std::fs::write(
            &config_path,
            format!("[channels.telegram]\nenabled = {}\n", val),
        )
        .unwrap();

        if tokio::time::timeout(std::time::Duration::from_millis(3000), rx.recv())
            .await
            .is_ok()
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    }

    assert!(
        call_count.load(Ordering::Relaxed) >= 1,
        "callback should have fired at least once after file changes (got {} calls)",
        call_count.load(Ordering::Relaxed)
    );
}

/// Atomic saves (write a temp file, then rename over the target) are how
/// editors and our own `toml_edit` writes persist config. They change the
/// file's inode, so a file-level watch silently misses them. The production
/// watcher fixes this by watching the DIRECTORY and filtering by filename —
/// this test pins that behavior (a rename over config.toml is detected).
#[tokio::test]
async fn atomic_save_via_rename_is_detected_with_dir_watch() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_path_buf();
    let config_path = dir.join("config.toml");
    std::fs::write(&config_path, "[agent]\n").unwrap();

    let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(4);

    drop({
        let dir = dir.clone();
        tokio::task::spawn_blocking(move || {
            let (etx, erx) = std::sync::mpsc::channel();
            let mut watcher =
                notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                    if let Ok(event) = res {
                        // Mirror production: react only to the three config files.
                        let relevant = event.paths.iter().any(|p| {
                            matches!(
                                p.file_name().and_then(|n| n.to_str()),
                                Some("config.toml" | "keys.toml" | "commands.toml")
                            )
                        });
                        if relevant {
                            let _ = etx.send(());
                        }
                    }
                })
                .unwrap();
            // Watch the DIRECTORY, like production does.
            let _ = watcher.watch(&dir, notify::RecursiveMode::NonRecursive);
            let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
            loop {
                let remaining = end.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() {
                    break;
                }
                match erx.recv_timeout(remaining.min(std::time::Duration::from_millis(200))) {
                    Ok(_) => {
                        let _ = tx.try_send(());
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                }
            }
        })
    });

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let mut detected = false;
    for i in 0..3 {
        // Write to a temp file, then rename over the target — the atomic-save
        // pattern a file-level watch would miss.
        let tmp_path = dir.join(format!("config.toml.tmp{i}"));
        std::fs::write(
            &tmp_path,
            format!("[agent]\ncontext_limit = {}\n", 200_000 + i),
        )
        .unwrap();
        std::fs::rename(&tmp_path, &config_path).unwrap();
        if tokio::time::timeout(std::time::Duration::from_millis(3000), rx.recv())
            .await
            .is_ok()
        {
            detected = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    }

    assert!(
        detected,
        "directory watch must detect an atomic save (write temp + rename over config.toml)"
    );
}
