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

/// `" (profile: NAME)"` when a named profile is active, else `""` — appended to
/// config-reload alerts so a multi-profile operator knows WHICH profile's config
/// was rejected (#534).
fn profile_suffix() -> String {
    profile_suffix_from(crate::config::profile::active_profile())
}

/// Pure core of [`profile_suffix`], for tests.
pub(crate) fn profile_suffix_from(profile: Option<&str>) -> String {
    match profile {
        Some(name) if !name.is_empty() && name != "default" => format!(" (profile: {name})"),
        _ => String::new(),
    }
}

/// Callback fired with a user-facing message when a hot reload does NOT cleanly
/// apply the on-disk config: either it recovered from last-known-good (the file
/// failed to parse) or the load failed entirely and the running config is kept.
/// Without this the failure was log-only, so an operator editing `config.toml`
/// had no signal that their change was rejected and the process kept serving
/// the old provider set (#534, mirror of upstream #517).
pub type ReloadNotify = Arc<dyn Fn(String) + Send + Sync>;

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
pub fn spawn(
    callbacks: Vec<ReloadCallback>,
    notify: Option<ReloadNotify>,
) -> tokio::task::JoinHandle<()> {
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

            match Config::load_with_status() {
                Ok((new_config, status)) => {
                    tracing::info!(
                        "ConfigWatcher: reloaded — firing {} callback(s)",
                        callbacks.len()
                    );
                    // If load() had to FALL BACK to last-known-good, config.toml
                    // itself is broken right now. Do NOT snapshot it over the good
                    // copy — a raw copy of a broken config poisons recovery, which
                    // is exactly how a malformed edit flipped auto-always (yolo)
                    // users into tool-approval prompts: the broken file AND its
                    // snapshot both failed to load, so the approval check had no
                    // valid config and defaulted to "ask". Keep the existing
                    // snapshot and run on the recovered values until config.toml
                    // parses cleanly again.
                    if status.recovered {
                        // Say WHAT failed, not just that something did (#909).
                        let reason = status
                            .recovery_reason
                            .clone()
                            .unwrap_or_else(|| "no reason recorded".to_string());
                        // "line 1, column 1" is the signature of reading an
                        // EMPTY file, not of a syntax error in the user's
                        // content. Config writes are not atomic, so a reader
                        // that lands between truncate and write sees zero bytes.
                        // Telling the user to fix a file that is already valid
                        // sends them hunting for a typo that does not exist.
                        let transient = reason.contains("line 1, column 1");
                        tracing::warn!(
                            "ConfigWatcher: load fell back to last-known-good                              (transient={transient}) — snapshot left untouched. Reason: {reason}"
                        );
                        if let Some(ref notify) = notify {
                            let path = base.join("config.toml");
                            notify(if transient {
                                format!(
                                    "⚠️ Config reload{} read {} mid-write and saw an empty file,                                      so it is running on the previous config for now. Your file                                      is almost certainly fine — this is a write race, not a typo.                                      Touch the file to retry. Details: {reason}",
                                    profile_suffix(),
                                    path.display(),
                                )
                            } else {
                                format!(
                                    "⚠️ Config reload{} failed — running on the previous config,                                      so your on-disk edits are NOT active.\n\nFile: {}\nError:                                      {reason}\n\nFix that and save; hot-reload applies                                      automatically.",
                                    profile_suffix(),
                                    path.display(),
                                )
                            });
                        }
                    } else {
                        // config.toml changed AND parsed cleanly — the real
                        // "last known good" moment. Snapshot it now (debounced, so
                        // once per edit) so recovery always has the latest valid
                        // config, instead of a once-per-process snapshot.
                        crate::config::save_last_good_config();
                    }
                    // Refresh the in-memory mirror so Config::current() readers
                    // see the new values without touching disk.
                    Config::set_current(new_config.clone());
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
                    if let Some(ref notify) = notify {
                        notify(format!(
                            "⚠️ config reload failed{} — keeping the running config. Error: {}. \
                             Fix {} and save; hot-reload will apply automatically.",
                            profile_suffix(),
                            e,
                            base.join("config.toml").display(),
                        ));
                    }
                }
            }
        }

        tracing::info!("ConfigWatcher: stopped");
    })
}
