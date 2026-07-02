//! End-to-end regression for #264: `/respond_to <mode>` issued from a group
//! must persist a `[channels.telegram.groups.<chat_id>]` override to
//! config.toml, and a fresh `Config::load()` (what a restart does) must read
//! it back and resolve it for that group.
//!
//! The production symptom looked like the setting was wiped on restart. It
//! never was: the write path is format-preserving and restart-safe. The
//! setting was simply never written, because the group command had been
//! autocompleted to another bot's handle (so this bot ignored it) and the
//! fallback @handle strip used to discard the command's argument (#265).

use crate::channels::commands::handle_respond_to;
use crate::config::{Config, RespondTo};

struct HomeGuard {
    prev_home: Option<std::ffi::OsString>,
    prev_userprofile: Option<std::ffi::OsString>,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl HomeGuard {
    fn new(temp_home: &std::path::Path) -> Self {
        let lock = crate::tests::HOME_ENV_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let prev_home = std::env::var_os("HOME");
        let prev_userprofile = std::env::var_os("USERPROFILE");
        // SAFETY: HOME_ENV_LOCK serializes access for the duration of `_lock`.
        unsafe {
            std::env::set_var("HOME", temp_home);
            std::env::set_var("USERPROFILE", temp_home);
        }
        Self {
            prev_home,
            prev_userprofile,
            _lock: lock,
        }
    }
}

impl Drop for HomeGuard {
    fn drop(&mut self) {
        // SAFETY: still holding HOME_ENV_LOCK via `_lock`.
        unsafe {
            match &self.prev_home {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
            match &self.prev_userprofile {
                Some(v) => std::env::set_var("USERPROFILE", v),
                None => std::env::remove_var("USERPROFILE"),
            }
        }
    }
}

// Unix-only for the same reason as other HOME-override config tests: on
// Windows `dirs::home_dir()` ignores env vars, so the guard has no effect.
#[cfg(unix)]
#[tokio::test]
async fn respond_to_in_group_persists_and_survives_reload() {
    let tmp = tempfile::tempdir().unwrap();
    let _guard = HomeGuard::new(tmp.path());
    let home = tmp.path().join(".opencrabs");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        home.join("config.toml"),
        "[channels.telegram]\nenabled = true\nrespond_to = \"mention\"\n",
    )
    .unwrap();

    let chat_id = "-1001234567890";
    let reply = handle_respond_to("all", Some(chat_id)).await;
    assert!(
        reply.starts_with("✅"),
        "expected a success reply, got: {reply}"
    );

    // The groups override is on disk, format preserved.
    let raw = std::fs::read_to_string(home.join("config.toml")).unwrap();
    assert!(
        raw.contains("groups") && raw.contains(chat_id),
        "config.toml is missing the groups override:\n{raw}"
    );

    // A fresh load (what a restart does) resolves the override.
    let cfg = Config::load().expect("config must reload after the group write");
    let group = cfg
        .channels
        .telegram
        .groups
        .get(chat_id)
        .expect("groups entry must persist across reload");
    assert!(matches!(group.respond_to, Some(RespondTo::All)));
    // Channel-level setting is untouched.
    assert!(matches!(
        cfg.channels.telegram.respond_to,
        RespondTo::Mention
    ));
}

// A DM (no chat id) keeps writing the channel-level key, not a group entry.
#[cfg(unix)]
#[tokio::test]
async fn respond_to_in_dm_writes_channel_level() {
    let tmp = tempfile::tempdir().unwrap();
    let _guard = HomeGuard::new(tmp.path());
    let home = tmp.path().join(".opencrabs");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        home.join("config.toml"),
        "[channels.telegram]\nenabled = true\nrespond_to = \"mention\"\n",
    )
    .unwrap();

    let reply = handle_respond_to("all", None).await;
    assert!(
        reply.starts_with("✅"),
        "expected a success reply, got: {reply}"
    );

    let cfg = Config::load().expect("config must reload");
    assert!(matches!(cfg.channels.telegram.respond_to, RespondTo::All));
    assert!(cfg.channels.telegram.groups.is_empty());
}
