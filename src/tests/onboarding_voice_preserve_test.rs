//! Voice-flag persistence regressions (#1233).
//!
//! Two writer bugs used to eat `[providers.stt.*]` / `[providers.tts.*]`
//! enablement behind the user's back:
//! 1. `/onboard:voice stt groq <KEY>` saved the key and claimed success
//!    without ever writing `enabled = true`;
//! 2. the wizard's `apply_config()` recomputed every voice flag from page
//!    state whenever the run reached Complete, so a full wizard pass that
//!    never touched voice stomped live disk truth with defaults.
//!
//! These lock both shut. Live-reload of the runtime gate itself is covered
//! by `stt_fallback_chain_test.rs` / `tts_fallback_chain_test.rs`.

use crate::config::profile::with_home_override;
use crate::tui::onboarding::{OnboardingStep, OnboardingWizard, SttProvider};

fn temp_home() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let home = dir.path().join(".opencrabs");
    std::fs::create_dir_all(&home).expect("create .opencrabs");
    (dir, home)
}

fn flag(home: &std::path::Path, segments: &[&str], key: &str) -> Option<bool> {
    let raw = std::fs::read_to_string(home.join("config.toml")).ok()?;
    let doc: toml_edit::DocumentMut = raw.parse().expect("valid TOML");
    // Walk as &dyn TableLike so each nested get() keeps the same type;
    // as_table_like() returns None on absence instead of panicking like [].
    let mut table: &dyn toml_edit::TableLike = doc.as_table();
    for seg in segments {
        table = table.get(seg)?.as_table_like()?;
    }
    match table.get(key)? {
        toml_edit::Item::Value(v) => v.as_bool().or_else(|| v.as_str().map(|s| s == "true")),
        _ => None,
    }
}

// ─── Bug 1: /onboard:voice stt groq must persist enablement ────────────────

#[test]
fn groq_voice_onboard_persists_enabled_true() {
    let (_temp, home) = temp_home();
    with_home_override(home.clone(), || {
        let result =
            crate::brain::tools::slash_onboard::onboard_voice("stt groq groq-test-key-123")
                .expect("dispatch succeeds");
        assert!(
            format!("{result:?}").contains("success"),
            "onboard reports success: {result:?}"
        );

        let raw = std::fs::read_to_string(home.join("config.toml")).expect("config.toml written");
        let doc: toml_edit::DocumentMut = raw.parse().expect("valid TOML");
        assert_eq!(
            doc["providers"]["stt"]["groq"]["enabled"].as_bool(),
            Some(true),
            "the arm that promises 'enabled' must actually write enabled=true"
        );
    });
}

#[test]
fn voice_flag_round_trips_through_disk_reload() {
    let (_temp, home) = temp_home();
    with_home_override(home.clone(), || {
        crate::config::Config::write_key("providers.stt.groq", "enabled", "true")
            .expect("flag written");

        // Fresh read of the file, not an in-memory cache: what any later
        // process or a live reload would observe.
        assert_eq!(
            flag(&home, &["providers", "stt", "groq"], "enabled"),
            Some(true)
        );
    });
}

// ─── Bug 2: Complete must not stomp untouched voice flags ──────────────────

#[test]
fn complete_without_touching_voice_preserves_disk_voice_flags() {
    let (_temp, home) = temp_home();
    with_home_override(home.clone(), || {
        // Disk truth: user's voice was already configured elsewhere.
        crate::config::Config::write_key("providers.stt.groq", "enabled", "true").unwrap();
        crate::config::Config::write_key("providers.tts.openai", "enabled", "true").unwrap();

        // A wizard run that never visits VoiceSetup still reaches Complete.
        let w = OnboardingWizard {
            step: OnboardingStep::Complete,
            ..OnboardingWizard::default()
        };
        w.apply_config().expect("apply succeeds");

        assert_eq!(
            flag(&home, &["providers", "stt", "groq"], "enabled"),
            Some(true),
            "Complete must not stomp providers.stt.groq.enabled when voice was never touched"
        );
        assert_eq!(
            flag(&home, &["providers", "tts", "openai"], "enabled"),
            Some(true),
            "Complete must not stomp providers.tts.openai.enabled when voice was never touched"
        );
    });
}

#[test]
fn complete_after_touching_voice_writes_current_selection() {
    let (_temp, home) = temp_home();
    with_home_override(home.clone(), || {
        let mut w = OnboardingWizard {
            step: OnboardingStep::VoiceSetup,
            ..OnboardingWizard::default()
        };

        // The user picks Groq and leaves the step through normal navigation:
        // that transition is what marks the run as voice-touched.
        w.stt_provider = SttProvider::Groq;
        w.groq_api_key_input = "key".into();
        w.next_step();

        assert!(w.voice_step_touched, "leaving VoiceSetup marks the run");
        w.step = OnboardingStep::Complete;
        w.apply_config().expect("apply succeeds");

        assert_eq!(
            flag(&home, &["providers", "stt", "groq"], "enabled"),
            Some(true),
            "an explicit voice selection IS written at Complete"
        );
    });
}
