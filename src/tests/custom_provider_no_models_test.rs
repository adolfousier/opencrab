//! Pin the "custom provider with no configured models returns empty
//! models + help text" behaviour added 2026-05-28.
//!
//! Pre-fix: a custom provider with neither `default_model` nor a
//! populated `models` list rendered a single inline-keyboard button
//! labeled "unknown (no models configured)". Clicking it called
//! `set_session_model` with that literal string and the agent silently
//! broke. User report: Telegram model switch for `custom:qwen-mlx`
//! (freshly merge-created from keys.toml) appeared to do nothing.
//!
//! Post-fix: empty `models` list + help text body. The channel handler
//! shows the help text instead of rendering an inert button.
//!
//! `models_for_provider` is tightly coupled to `Config::load()` so we
//! exercise the contract via a temp-config + HOME-override harness.

use crate::channels::commands::models_for_provider;

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
        // `dirs::home_dir()` reads HOME on Unix and USERPROFILE on Windows
        // (with registry fallback) — set both so the override works on both.
        // Without USERPROFILE the Windows CI test reads the runner's real
        // profile, which lacks our temp config and falls through to a
        // different code path. 2026-05-29 Windows CI fix.
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
        match self.prev_home.take() {
            Some(v) => unsafe { std::env::set_var("HOME", v) },
            None => unsafe { std::env::remove_var("HOME") },
        }
        match self.prev_userprofile.take() {
            Some(v) => unsafe { std::env::set_var("USERPROFILE", v) },
            None => unsafe { std::env::remove_var("USERPROFILE") },
        }
    }
}

fn write_temp_home(config_toml: &str) -> tempfile::TempDir {
    use std::io::Write;
    let dir = tempfile::tempdir().expect("tempdir");
    let opencrabs = dir.path().join(".opencrabs");
    std::fs::create_dir_all(&opencrabs).expect("create .opencrabs");
    let path = opencrabs.join("config.toml");
    let mut f = std::fs::File::create(&path).expect("create config");
    f.write_all(config_toml.as_bytes()).expect("write config");
    // Empty keys.toml — config we test sets api_key inline.
    std::fs::write(opencrabs.join("keys.toml"), b"").expect("write keys");
    dir
}

#[tokio::test]
async fn empty_custom_provider_returns_empty_models_and_help_text() {
    let temp = write_temp_home(
        r#"
[providers.custom.qwen-mlx]
enabled = true
base_url = "http://localhost:8080/v1"
api_key = "test-key"
# Deliberately no default_model and no models list
"#,
    );
    let _guard = HomeGuard::new(temp.path());

    let resp = models_for_provider("custom:qwen-mlx").await;

    assert!(
        resp.models.is_empty(),
        "custom provider with no default_model + empty models list must return \
         empty models, NOT a placeholder button labeled 'unknown (no models \
         configured)'. Got models: {:?}",
        resp.models
    );
    assert!(
        resp.text.contains("No models configured"),
        "must show 'No models configured' help text, got: {}",
        resp.text
    );
    assert!(
        resp.text.contains("default_model"),
        "help text must mention default_model so the user knows what to add"
    );
    assert!(
        resp.text.contains("[providers.custom.qwen-mlx]"),
        "help text must include the TOML section for the specific provider, got: {}",
        resp.text
    );
}

#[tokio::test]
async fn custom_provider_with_default_model_returns_real_button() {
    let temp = write_temp_home(
        r#"
[providers.custom.qwen-mlx]
enabled = true
base_url = "http://localhost:8080/v1"
api_key = "test-key"
default_model = "qwen3-7b-mlx-4bit"
"#,
    );
    let _guard = HomeGuard::new(temp.path());

    let resp = models_for_provider("custom:qwen-mlx").await;

    assert!(
        !resp.models.is_empty(),
        "custom provider WITH default_model must produce a real model list"
    );
    assert!(
        resp.models.contains(&"qwen3-7b-mlx-4bit".to_string()),
        "real default_model must appear in the picker, got: {:?}",
        resp.models
    );
    assert!(
        !resp.text.contains("No models configured"),
        "must NOT show the empty-config help text when default_model is set"
    );
    assert!(
        !resp.text.contains("unknown (no models configured)"),
        "must NEVER include the pre-fix placeholder string"
    );
}
