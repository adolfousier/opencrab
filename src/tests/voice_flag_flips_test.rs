//! The reload diff that names a voice flag switched off (#1399).

use crate::config::Config;
use crate::config::voice_flag_flips::voice_flags_switched_off;

fn cfg(toml: &str) -> Config {
    toml::from_str(toml).expect("config toml parses")
}

#[test]
fn a_flag_that_went_off_is_named_with_its_dotted_key() {
    let prev =
        cfg("[providers.tts.openai]\nenabled = true\n[providers.stt.groq]\nenabled = true\n");
    let next =
        cfg("[providers.tts.openai]\nenabled = false\n[providers.stt.groq]\nenabled = true\n");
    assert_eq!(
        voice_flags_switched_off(&prev, &next),
        vec!["providers.tts.openai.enabled"]
    );
}

#[test]
fn a_table_that_vanished_counts_as_switched_off() {
    let prev = cfg("[providers.stt.local]\nenabled = true\n");
    let next = cfg("[agent]\n");
    assert_eq!(
        voice_flags_switched_off(&prev, &next),
        vec!["providers.stt.local.enabled"]
    );
}

#[test]
fn the_reported_wholesale_reset_names_every_engine_that_was_on() {
    // What the 2026-09-05 rewrite did: every engine off at once.
    let prev = cfg(
        "[providers.stt.groq]\nenabled = true\n[providers.stt.local]\nenabled = true\n\
         [providers.tts.openai]\nenabled = true\n",
    );
    let next = cfg(
        "[providers.stt.groq]\nenabled = false\n[providers.stt.local]\nenabled = false\n\
         [providers.tts.openai]\nenabled = false\n",
    );
    assert_eq!(
        voice_flags_switched_off(&prev, &next),
        vec![
            "providers.stt.groq.enabled",
            "providers.stt.local.enabled",
            "providers.tts.openai.enabled",
        ]
    );
}

#[test]
fn switching_on_or_unchanged_reports_nothing() {
    let prev = cfg("[providers.tts.openai]\nenabled = false\n");
    let next =
        cfg("[providers.tts.openai]\nenabled = true\n[providers.tts.local]\nenabled = true\n");
    assert!(voice_flags_switched_off(&prev, &next).is_empty());
    assert!(voice_flags_switched_off(&next, &next).is_empty());
}
