//! `[providers.xiaomi]` materializes to the canonical keyless defaults when the
//! TOML omits the section, so a config that predates the provider (or a fresh
//! `/evolve` that never appended it) still gets a working, selectable Xiaomi
//! with no manual edits (#194). A section the user did write is left exactly as
//! written — the default never clobbers explicit values.

use crate::config::ProviderConfigs;
use crate::config::xiaomi_provider_defaults;

#[test]
fn missing_xiaomi_section_deserializes_to_canonical_defaults() {
    // No [xiaomi] table at all — the bug case (old / hand-edited config).
    let cfgs: ProviderConfigs = toml::from_str("").expect("empty providers should parse");
    let xiaomi = cfgs.xiaomi.expect("xiaomi must default to Some, not None");

    assert!(xiaomi.enabled, "default provider is enabled");
    assert_eq!(xiaomi.default_model.as_deref(), Some("mimo-v2.5-pro"));
    assert!(
        xiaomi.api_key.is_none(),
        "keyless — the proxy supplies the key"
    );
    assert!(
        xiaomi.base_url.is_none(),
        "base_url stays None so the factory uses the proxy default"
    );
    assert_eq!(xiaomi.models, xiaomi_provider_defaults().models);
    // MiMo v2.5 is multimodal, so vision (analyze_image) routes to it natively
    // via ProviderVisionTool — no Gemini key required for the keyless promo.
    assert_eq!(xiaomi.vision_model.as_deref(), Some("mimo-v2.5-pro"));
    // Capped at 200k despite MiMo's ~1M — quality degrades past ~200-300k and
    // transparent compaction already gives effectively-infinite memory.
    assert_eq!(
        xiaomi.context_window,
        Some(200_000),
        "Xiaomi must default to a 200k context window"
    );
}

#[test]
fn present_xiaomi_section_is_left_untouched() {
    // A user who wrote their own section keeps their values; the field default
    // only fires when the whole section is absent.
    let toml =
        "[xiaomi]\nenabled = true\ndefault_model = \"mimo-v2-flash\"\napi_key = \"user-key\"\n";
    let cfgs: ProviderConfigs = toml::from_str(toml).expect("parse");
    let xiaomi = cfgs.xiaomi.expect("present");

    assert_eq!(xiaomi.default_model.as_deref(), Some("mimo-v2-flash"));
    assert_eq!(xiaomi.api_key.as_deref(), Some("user-key"));
    assert!(
        xiaomi.models.is_empty(),
        "default model list must NOT be grafted onto an explicit section"
    );
}

#[test]
fn programmatic_default_keeps_xiaomi_none() {
    // The serde default is deserialization-only; an in-memory Default::default()
    // (used throughout tests) is unaffected, so blast radius stays small.
    assert!(ProviderConfigs::default().xiaomi.is_none());
}

/// Pin the keyless-window boundary so the cutoff can't drift back to closing a
/// day early again. The opencrabs × Xiaomi collab ends 2026-06-27 00:00 UTC, so
/// the 26th must still be open and the 27th must be closed.
#[test]
fn keyless_window_closes_at_2026_06_27_00_utc_not_a_day_early() {
    use crate::brain::provider::factory::xiaomi_keyless_open_on;
    let d = |s: &str| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();

    assert!(xiaomi_keyless_open_on(d("2026-06-25")), "25th: open");
    assert!(
        xiaomi_keyless_open_on(d("2026-06-26")),
        "26th must still be open — collab runs through the 26th (ends 27th 00:00 UTC)"
    );
    assert!(
        !xiaomi_keyless_open_on(d("2026-06-27")),
        "27th: closed — window ends at 27th 00:00 UTC"
    );
}
