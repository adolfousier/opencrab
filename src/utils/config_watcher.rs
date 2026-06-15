//! Config hot-reload watcher.
//!
//! Watches the `~/.opencrabs/` directory and reacts to changes of
//! `config.toml`, `keys.toml`, and `commands.toml`. On any modification it
//! re-loads the full `Config` and fires all registered callbacks. Watching the
//! directory (rather than the files directly) is deliberate: atomic saves
//! rename a temp file over the target, which a file-level watch would miss.
//!
//! Designed to be extended: register any channel state update or command reload
//! by pushing a `ReloadCallback` via `spawn()`.

use crate::config::{Config, opencrabs_home};
use notify::{RecursiveMode, Watcher};
use std::sync::Arc;
use std::time::Duration;

/// Callback fired on every successful config reload.
pub type ReloadCallback = Arc<dyn Fn(Config) + Send + Sync>;

/// Spawn a background task that watches config files and fires callbacks on change.
/// Debounces rapid file-save events (300 ms window) before reloading.
///
/// # Example
/// ```ignore
/// config_watcher::spawn(vec![
///     Arc::new(move |cfg| {
///         let state = telegram_state.clone();
///         tokio::spawn(async move {
///             state.update_allowed_users(cfg.channels.telegram.allowed_users).await;
///         });
///     }),
/// ]);
/// ```
pub fn spawn(callbacks: Vec<ReloadCallback>) -> tokio::task::JoinHandle<()> {
    tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Handle::current();
        let base = opencrabs_home();

        let (tx, rx) = std::sync::mpsc::channel();

        let mut watcher =
            match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                if let Ok(event) = res {
                    // The directory watch also sees the SQLite DB and other churn in
                    // ~/.opencrabs/, which would trigger a reload storm. React only
                    // to our three config files.
                    let relevant = event.paths.iter().any(|p| {
                        matches!(
                            p.file_name().and_then(|n| n.to_str()),
                            Some("config.toml" | "keys.toml" | "commands.toml")
                        )
                    });
                    if relevant {
                        let _ = tx.send(event);
                    }
                }
            }) {
                Ok(w) => w,
                Err(e) => {
                    tracing::error!("ConfigWatcher: failed to create watcher: {}", e);
                    return;
                }
            };

        // Watch the DIRECTORY, not the individual files. Editors and our own
        // toml_edit writes save atomically (write a temp file, then rename over
        // the target), which changes the file's inode. A file-level watch stays
        // bound to the now-deleted inode and silently misses every later edit —
        // the cause of "saved config but the daemon (or TUI) didn't hot-reload,
        // needed a restart." A directory watch survives renames and reliably
        // catches every save.
        if base.exists()
            && let Err(e) = watcher.watch(&base, RecursiveMode::NonRecursive)
        {
            tracing::error!("ConfigWatcher: cannot watch {:?}: {}", base, e);
            return;
        }

        tracing::info!(
            "ConfigWatcher: watching config.toml, keys.toml and commands.toml in {:?}",
            base
        );

        let debounce = Duration::from_millis(300);

        while rx.recv().is_ok() {
            // Drain further events within the debounce window
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

            match Config::load() {
                Ok(new_config) => {
                    tracing::info!(
                        "ConfigWatcher: reloaded — firing {} callback(s)",
                        callbacks.len()
                    );
                    // The config just changed AND parsed cleanly — this is the
                    // real "last known good" moment. Snapshot it now (debounced,
                    // so once per edit) so recovery always has the latest valid
                    // config, instead of a months-old once-per-process snapshot.
                    crate::config::save_last_good_config();
                    for cb in &callbacks {
                        let cb = cb.clone();
                        let cfg = new_config.clone();
                        rt.spawn(async move { cb(cfg) });
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "ConfigWatcher: reload failed, keeping current config: {}",
                        e
                    );
                }
            }
        }

        tracing::info!("ConfigWatcher: stopped");
    })
}
