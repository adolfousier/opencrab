//! Regression tests for the home-anchor + tilde-expansion rule rendered
//! under the `Working directory:` line in the system brain.
//!
//! Background: 2026-04-26 we collapsed `$HOME → ~` in the runtime info
//! (commit 8801ce0) for privacy / cache / token reasons. That removed
//! the literal username from the prompt entirely, and the model
//! started inventing one (`/Users/adolfo/...`, the user's first name
//! from git) whenever it needed an absolute path. The Telegram channel
//! was the loudest example — every `bash` call to a sister project
//! failed with the wrong username.
//!
//! Both render paths (`build_system_brain` and `build_core_brain`) must
//! emit the anchor lines so neither code path regresses again.

use crate::brain::prompt_builder::{BrainLoader, RuntimeInfo};
use tempfile::TempDir;

fn loader() -> (TempDir, BrainLoader) {
    let dir = TempDir::new().expect("tempdir");
    let loader = BrainLoader::new(dir.path().to_path_buf());
    (dir, loader)
}

fn runtime_info_with_collapsed_wd() -> RuntimeInfo {
    RuntimeInfo {
        model: Some("test-model".to_string()),
        provider: Some("test-provider".to_string()),
        // Pre-collapsed form — that's what the CLI sites pass after
        // running `collapse_home` on the absolute working directory.
        working_directory: Some("~/srv/rs/opencrabs".to_string()),
    }
}

#[test]
fn full_brain_renders_home_anchor_when_wd_present() {
    let (_dir, loader) = loader();
    let info = runtime_info_with_collapsed_wd();
    let prompt = loader.build_system_brain(Some(&info), None);

    // The collapsed wd is still visible (the original feature)
    assert!(prompt.contains("Working directory: ~/srv/rs/opencrabs"));
    // Home anchor is emitted right under it so the model has ground
    // truth instead of guessing the username.
    let home = dirs::home_dir().expect("home dir");
    assert!(
        prompt.contains(&format!("Home: {}", home.display())),
        "home anchor missing — prompt should include `Home: {}`",
        home.display()
    );
    assert!(
        prompt.contains("the '~' in paths above expands to this"),
        "home-anchor explainer missing"
    );
}

#[test]
fn full_brain_renders_path_expansion_rule() {
    let (_dir, loader) = loader();
    let info = runtime_info_with_collapsed_wd();
    let prompt = loader.build_system_brain(Some(&info), None);

    // The actual rule the model needs to follow.
    assert!(
        prompt.contains("Path expansion:"),
        "missing 'Path expansion:' header"
    );
    assert!(
        prompt.contains("the shell expands `~` for you"),
        "missing the 'shell expands ~ for you' guidance"
    );
    assert!(
        prompt.contains("Do NOT substitute `/Users/<name>/...` yourself"),
        "missing the explicit 'do not substitute' rule that addresses the \
         /Users/adolfo regression"
    );
}

#[test]
fn core_brain_renders_home_anchor_when_wd_present() {
    // build_core_brain is a separate render path used in lean prompts —
    // it must include the same anchors or Telegram (which uses it
    // sometimes via the agent service) regresses.
    let (_dir, loader) = loader();
    let info = runtime_info_with_collapsed_wd();
    let prompt = loader.build_core_brain(Some(&info), None);

    assert!(prompt.contains("Working directory: ~/srv/rs/opencrabs"));
    let home = dirs::home_dir().expect("home dir");
    assert!(prompt.contains(&format!("Home: {}", home.display())));
    assert!(prompt.contains("Path expansion:"));
    assert!(prompt.contains("the shell expands `~` for you"));
}

#[test]
fn no_anchor_when_working_directory_is_none() {
    // If runtime_info is present but working_directory is None, we
    // shouldn't emit an orphaned `Home:` / expansion rule with nothing
    // to anchor.
    let (_dir, loader) = loader();
    let info = RuntimeInfo {
        model: Some("test-model".to_string()),
        provider: None,
        working_directory: None,
    };
    let prompt = loader.build_system_brain(Some(&info), None);

    assert!(
        !prompt.contains("Home: "),
        "Home anchor should not appear without a working directory"
    );
    assert!(
        !prompt.contains("Path expansion:"),
        "expansion rule should not appear without a working directory"
    );
}

#[test]
fn no_anchor_when_runtime_info_absent() {
    // When the caller passes None for runtime_info we shouldn't render
    // the section at all (let alone the anchor).
    let (_dir, loader) = loader();
    let prompt = loader.build_system_brain(None, None);

    assert!(!prompt.contains("--- Runtime Info ---"));
    assert!(!prompt.contains("Home: "));
    assert!(!prompt.contains("Path expansion:"));
}

/// Issue #183: the agent hallucinated its version because the system prompt
/// carried no ground truth. Both Runtime Info render paths must now surface
/// the compile-time `CARGO_PKG_VERSION` so a direct "what version are you?"
/// is answerable without the /doctor workaround.
#[test]
fn both_render_paths_include_the_binary_version() {
    let (_dir, loader) = loader();
    let info = runtime_info_with_collapsed_wd();
    let version = env!("CARGO_PKG_VERSION");

    let system = loader.build_system_brain(Some(&info), None);
    assert!(
        system.contains(&format!("OpenCrabs version: v{version}")),
        "build_system_brain must surface the running binary's version (#183)"
    );

    let core = loader.build_core_brain(Some(&info), None);
    assert!(
        core.contains(&format!("OpenCrabs version: v{version}")),
        "build_core_brain (lean prompt) must surface the version too (#183)"
    );
}
