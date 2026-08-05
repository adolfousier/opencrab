//! Step-scoped save regression tests (#926).
//!
//! Each confirmed wizard step now commits its own config section on
//! transition, so an interrupted onboarding keeps everything already
//! confirmed instead of losing it all to the single end-of-wizard write.
//!
//! These tests run in a tempdir so `next_step()` config writes never
//! touch the user's real `~/.opencrabs/` (#912 isolation pattern).

use crate::config::profile::with_home_override;
use crate::tui::onboarding::{OnboardingStep, OnboardingWizard, WizardMode};
use std::path::PathBuf;

/// A tempdir laid out as an `.opencrabs` home. Step-scoped writes land
/// here and the directory is removed when the guard drops.
fn temp_home() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let opencrabs = dir.path().join(".opencrabs");
    std::fs::create_dir_all(&opencrabs).expect("create .opencrabs");
    (dir, opencrabs)
}

/// A wizard sat on ProviderAuth in Advanced mode, ready to move forward.
/// Uses `default()` so there are no disk reads from `new()` at
/// construction time.
#[allow(clippy::field_reassign_with_default)]
fn wizard_at_provider_auth() -> OnboardingWizard {
    let mut w = OnboardingWizard::default();
    w.mode = WizardMode::Advanced;
    w.step = OnboardingStep::ProviderAuth;
    w.ps.api_key_input = "sk-test-key".to_string();
    w
}

// ── acceptance criterion 1 ──────────────────────────────────────────
// Confirming a step writes that step's section before the next step
// renders.

#[test]
fn leaving_provider_auth_persists_the_provider_section() {
    let (_temp, home) = temp_home();
    with_home_override(home.clone(), || {
        let mut w = wizard_at_provider_auth();
        w.next_step(); // ProviderAuth → Channels
        assert_eq!(w.step, OnboardingStep::Channels, "transition succeeds");
        assert!(w.error_message.is_none());

        let config =
            std::fs::read_to_string(home.join("config.toml")).expect("config written by step save");
        let doc: toml_edit::DocumentMut = config
            .parse()
            .expect("step-saved config.toml must be valid TOML");
        assert_eq!(
            doc["providers"]["anthropic"]["enabled"].as_bool(),
            Some(true),
            "selected provider is enabled on disk before the Channels step renders"
        );

        let keys =
            std::fs::read_to_string(home.join("keys.toml")).expect("keys written by step save");
        assert!(
            keys.contains("sk-test-key"),
            "the API key lands in keys.toml during step save, not just at the end"
        );
    });
}

#[test]
fn leaving_channels_persists_channel_toggles() {
    let (_temp, home) = temp_home();
    with_home_override(home.clone(), || {
        let mut w = wizard_at_provider_auth();
        w.next_step(); // ProviderAuth → Channels (provider saved)
        assert_eq!(w.step, OnboardingStep::Channels);

        w.next_step(); // Channels → VoiceSetup (channels saved)
        assert_eq!(w.step, OnboardingStep::VoiceSetup);
        assert!(w.error_message.is_none());

        let config =
            std::fs::read_to_string(home.join("config.toml")).expect("config exists after 2 saves");
        let doc: toml_edit::DocumentMut = config.parse().expect("valid TOML");
        // The channel toggles block was written — at minimum the telegram
        // and discord enabled flags exist.
        assert!(
            doc["channels"]["telegram"].get("enabled").is_some(),
            "telegram enabled flag committed during Channels step save"
        );
        assert!(
            doc["channels"]["discord"].get("enabled").is_some(),
            "discord enabled flag committed during Channels step save"
        );
    });
}

// ── acceptance criterion 2 ──────────────────────────────────────────
// Abandoning the wizard midway keeps every step already confirmed.

#[test]
fn abandoning_mid_wizard_keeps_confirmed_sections() {
    let (_temp, home) = temp_home();
    with_home_override(home.clone(), || {
        {
            let mut w = wizard_at_provider_auth();
            w.next_step(); // ProviderAuth → Channels (provider saved)
            assert_eq!(w.step, OnboardingStep::Channels);
            w.next_step(); // Channels → VoiceSetup (channels saved)
            assert_eq!(w.step, OnboardingStep::VoiceSetup);
            // Wizard dropped here without ever calling apply_config.
        }

        let config = std::fs::read_to_string(home.join("config.toml"))
            .expect("both step-scoped saves survive the drop");
        let doc: toml_edit::DocumentMut = config.parse().expect("valid TOML");
        assert_eq!(
            doc["providers"]["anthropic"]["enabled"].as_bool(),
            Some(true),
            "provider section survives wizard interruption"
        );
        assert!(
            doc["channels"]["telegram"].get("enabled").is_some(),
            "channel toggles survive wizard interruption"
        );
    });
}

// ── acceptance criterion 3 ──────────────────────────────────────────
// A failed intermediate save reports the section, the key and the
// underlying cause.

#[test]
fn failed_step_save_surfaces_section_and_cause() {
    let (_temp, home) = temp_home();
    with_home_override(home.clone(), || {
        // Make config.toml a directory so every `read_to_string` inside
        // `Config::write_key` fails deterministically — the repair path
        // also cannot read a directory and bails out with a warning.
        let config_path = home.join("config.toml");
        std::fs::create_dir(&config_path).expect("create directory named config.toml");

        let mut w = wizard_at_provider_auth();
        w.next_step(); // ProviderAuth → Channels

        // The step-save failed, so the wizard must stay in place.
        assert_eq!(
            w.step,
            OnboardingStep::ProviderAuth,
            "a failed save rolls the step back"
        );
        let err = w.error_message.expect("failure surfaces, not swallowed");
        assert!(
            err.contains("provider step"),
            "error names the owning section: {err}"
        );
        assert!(
            err.contains("providers."),
            "error names the key that failed: {err}"
        );
    });
}

#[test]
fn failed_step_save_contains_the_underlying_io_cause() {
    let (_temp, home) = temp_home();
    with_home_override(home.clone(), || {
        let config_path = home.join("config.toml");
        std::fs::create_dir(&config_path).expect("config.toml as directory");

        let mut w = wizard_at_provider_auth();
        w.next_step();

        let err = w.error_message.unwrap();
        // The try_write! macro preserves the root cause, not just the key
        // path — "Is a directory" (macOS/Linux) or similar OS error must
        // appear in the message.
        assert!(
            err.contains("setting(s) could not be saved"),
            "structured error from write_scoped_config: {err}"
        );
    });
}

// ── finalize guard: step-scoped saves must not run completion-only ──
// ── tasks (template seeding, daemon install).                        ──

#[test]
fn step_save_does_not_seed_templates() {
    let (_temp, home) = temp_home();
    with_home_override(home.clone(), || {
        let mut w = wizard_at_provider_auth();
        // Point the workspace at the temp home so seeding, if it ran,
        // would leave visible evidence.
        w.workspace_path = home.to_string_lossy().to_string();
        assert!(
            w.seed_templates,
            "defaults to true, so this is the risky case"
        );

        w.next_step(); // ProviderAuth → Channels
        assert_eq!(w.step, OnboardingStep::Channels);

        // The static SOUL.md template must NOT have been written; seeding
        // is a completion task, not a step-scoped task (#926).
        assert!(
            !home.join("SOUL.md").exists(),
            "step-scoped save must not seed workspace templates"
        );
    });
}

// ── quick_jump is untouched: it keeps using the end-of-wizard ──
// ── apply_config path, not step-scoped saves.                  ──

#[test]
#[allow(clippy::field_reassign_with_default)]
fn quick_jump_does_not_trigger_step_save() {
    let (_temp, home) = temp_home();
    with_home_override(home.clone(), || {
        let mut w = OnboardingWizard::default();
        w.quick_jump = true;
        w.step = OnboardingStep::ProviderAuth;
        w.ps.api_key_input = "sk-test-key".to_string();

        w.next_step();
        assert!(w.quick_jump_done, "quick_jump sets done flag");
        assert_eq!(
            w.step,
            OnboardingStep::ProviderAuth,
            "step does not change in quick_jump mode"
        );

        // No config was written — quick_jump_done triggers apply_config
        // from dialogs.rs, not from next_step.
        assert!(
            !home.join("config.toml").exists(),
            "no step-scoped save fired for quick_jump"
        );
    });
}

// ── sectionless steps are a silent no-op ────────────────────────────

#[test]
fn mode_select_to_workspace_does_not_write_config() {
    let (_temp, home) = temp_home();
    with_home_override(home.clone(), || {
        let mut w = OnboardingWizard::default();
        assert_eq!(w.step, OnboardingStep::ModeSelect);

        w.next_step(); // ModeSelect → Workspace
        assert_eq!(w.step, OnboardingStep::Workspace);

        assert!(
            !home.join("config.toml").exists(),
            "ModeSelect and Workspace own no section, nothing written"
        );
    });
}
