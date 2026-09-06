//! Migration 2 (`[voice]` to `providers.stt.*` / `providers.tts.*`) must
//! honour a legacy `enabled = true` (#1399).
//!
//! The migration used `entry("enabled").or_insert(true)`, a no-op whenever
//! the provider table already carried `enabled = false`, so a config that
//! said voice was on came out of the migration with voice off and there
//! was no second chance: the `[voice]` table is deleted in the same pass.

use crate::config::Config;
use tempfile::TempDir;

fn migrate(contents: &str) -> (TempDir, toml::Value) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, contents).unwrap();
    Config::migrate_if_needed(&path);
    let doc: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    (dir, doc)
}

#[test]
fn legacy_tts_enabled_re_enables_a_disabled_openai_table() {
    let (_d, doc) = migrate(
        "[voice]\ntts_enabled = true\ntts_mode = \"api\"\ntts_voice = \"echo\"\n\n\
         [providers.tts.openai]\nenabled = false\n",
    );
    assert_eq!(
        doc["providers"]["tts"]["openai"]["enabled"].as_bool(),
        Some(true)
    );
    assert_eq!(
        doc["providers"]["tts"]["openai"]["voice"].as_str(),
        Some("echo")
    );
    assert!(doc.get("voice").is_none(), "the legacy table is consumed");
}

#[test]
fn legacy_tts_enabled_creates_the_openai_table_when_missing() {
    let (_d, doc) = migrate("[voice]\ntts_enabled = true\ntts_mode = \"api\"\n");
    assert_eq!(
        doc["providers"]["tts"]["openai"]["enabled"].as_bool(),
        Some(true)
    );
}

#[test]
fn legacy_local_stt_enabled_re_enables_a_disabled_local_table() {
    let (_d, doc) = migrate(
        "[voice]\nstt_enabled = true\nstt_mode = \"local\"\nlocal_stt_model = \"local-base\"\n\n\
         [providers.stt.local]\nenabled = false\nmodel = \"local-tiny\"\n",
    );
    assert_eq!(
        doc["providers"]["stt"]["local"]["enabled"].as_bool(),
        Some(true)
    );
    assert_eq!(
        doc["providers"]["stt"]["local"]["model"].as_str(),
        Some("local-tiny"),
        "an existing model choice is kept; only the flag is forced"
    );
}

#[test]
fn legacy_groq_stt_enabled_creates_the_groq_table_when_missing() {
    let (_d, doc) = migrate("[voice]\nstt_enabled = true\nstt_mode = \"api\"\n");
    assert_eq!(
        doc["providers"]["stt"]["groq"]["enabled"].as_bool(),
        Some(true)
    );
}

#[test]
fn legacy_disabled_voice_leaves_provider_flags_alone() {
    let (_d, doc) = migrate(
        "[voice]\ntts_enabled = false\nstt_enabled = false\n\n\
         [providers.tts.openai]\nenabled = false\n",
    );
    assert_eq!(
        doc["providers"]["tts"]["openai"]["enabled"].as_bool(),
        Some(false)
    );
    assert!(doc.get("voice").is_none());
}
