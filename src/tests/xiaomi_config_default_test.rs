//! `[providers.xiaomi]` is an ordinary provider section: absent from the TOML
//! means absent, present means exactly what the user wrote.
//!
//! It used to materialize a populated, enabled section whenever the TOML
//! omitted it (#194), a leftover from the window when MiMo shipped as the
//! keyless default provider. The picker never depended on it: `load_default_models`
//! reads every provider's catalogue out of the embedded `config.toml.example`,
//! which ships xiaomi disabled. What the default did instead was make every
//! config that never named xiaomi parse as though it had enabled one.

use crate::config::ProviderConfigs;

#[test]
fn missing_xiaomi_section_stays_missing() {
    // No [xiaomi] table at all: an old or hand-edited config, or one written
    // before the provider existed.
    let cfgs: ProviderConfigs = toml::from_str("").expect("empty providers should parse");

    assert!(
        cfgs.xiaomi.is_none(),
        "a config that never named xiaomi must not parse as having enabled it"
    );
}

#[test]
fn present_xiaomi_section_is_left_untouched() {
    // A user who wrote their own section keeps their values verbatim.
    let toml =
        "[xiaomi]\nenabled = true\ndefault_model = \"mimo-v2-flash\"\napi_key = \"user-key\"\n";
    let cfgs: ProviderConfigs = toml::from_str(toml).expect("parse");
    let xiaomi = cfgs.xiaomi.expect("present");

    assert_eq!(xiaomi.default_model.as_deref(), Some("mimo-v2-flash"));
    assert_eq!(xiaomi.api_key.as_deref(), Some("user-key"));
    assert!(
        xiaomi.models.is_empty(),
        "no model list is grafted onto an explicit section"
    );
}

#[test]
fn programmatic_default_keeps_xiaomi_none() {
    assert!(ProviderConfigs::default().xiaomi.is_none());
}
