//! Pins the section-resolver invariant for `save_provider_settings` in
//! `src/tui/app/dialogs.rs`.
//!
//! Regression: 2026-06-04. User opens `/models`, cursor sits on the
//! currently-active custom provider (`dialagram`). They navigate to the
//! "+ Add new custom" row. `reload_model_selector_custom_fields` correctly
//! clears `self.ps.custom_name = ""`. They start typing the new entry's
//! `base_url` field. Every keystroke fires `save_provider_settings` with
//! `close_dialog=false` (per-field merge save).
//!
//! Pre-fix, the section resolver's `""` arm rescued empty `custom_name`
//! with `config.providers.active_custom()` — which still pointed at
//! `dialagram`. The keystroke-driven save then did
//! `Config::write_key("providers.custom.dialagram", "base_url", <new-url>)`,
//! silently corrupting dialagram's section. The model field survived
//! because `write_key` is a per-key TOML merge.
//!
//! Concrete corruption observed in user's config.toml:
//! ```toml
//! [providers.custom.dialagram]
//! base_url = "https://api-inference.modelscope.ai/v1"   # overwritten
//! default_model = "qwen-3.7-max-thinking"               # untouched
//! ```
//!
//! The fix: never fall back to `active_custom()` in the section resolver.
//! Empty `custom_name` means "user is mid-draft, no name typed yet" — the
//! only safe action is to skip the write.

// The provider-save resolver moved out of the deleted ModelSelector dialog
// into the onboarding save path that /models now reuses.
const SAVE_SRC_RAW: &str = include_str!("../tui/onboarding/config.rs");

/// Strip `//` line comments so source-level invariant scans don't false-
/// match against the regression doc-comments that describe the bug they're
/// guarding against. Same approach as the
/// `approval_requests_are_not_routed_through_session_state_mut` sentinel
/// in `background_session_test.rs`.
fn save_src_code() -> String {
    SAVE_SRC_RAW
        .lines()
        .map(|line| {
            if let Some(idx) = line.find("//") {
                let before = &line[..idx];
                let quote_count = before.matches('"').count();
                if quote_count % 2 == 0 {
                    return before.trim_end().to_string();
                }
            }
            line.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn pre_fix_active_custom_fallback_pattern_is_absent() {
    // Exact code shape of the pre-fix bug: an `else-if-let` arm pulling
    // `active_custom()` and feeding it into the section name. This
    // specific pattern is what corrupted dialagram on 2026-06-04. Banning
    // the exact shape is brittle to formatting but a fair regression
    // sentinel — if a future refactor needs `active_custom()` for an
    // unrelated read it won't trip this, but a re-introduction of the
    // fallback would land textually-identical or near-identical code.
    let src = save_src_code();
    let forbidden_signatures = [
        "else if let Some((name, _)) = config.providers.active_custom()",
        "else if let Some((name, _)) = self.providers.active_custom()",
        ".active_custom() {\n                    name.to_string()",
    ];
    for sig in forbidden_signatures {
        assert!(
            !src.contains(sig),
            "save_provider_settings: forbidden fallback pattern re-introduced.\n\
             Signature: {sig:?}\n\
             This is the 2026-06-04 dialagram-corruption pattern. See \
             custom_provider_section_resolver_test doc for the full repro and the \
             reasoning behind banning the fallback."
        );
    }
}

#[test]
fn empty_custom_name_guard_precedes_section_format() {
    // `apply_config` builds the custom write target with
    // `custom_section = format!("providers.custom.{}", self.ps.custom_name);`.
    // An empty-name guard MUST appear BEFORE that assignment and disable the
    // provider write (`write_provider = false`) so an empty custom_name can
    // never format the literal section `providers.custom.` and corrupt
    // config.toml. Anchor on the first format assignment and require the
    // guard somewhere ahead of it.
    let src = save_src_code();
    let format_marker = "custom_section = format!(\"providers.custom.{}\", self.ps.custom_name)";
    let format_idx = src.find(format_marker).unwrap_or_else(|| {
        panic!(
            "expected the section-format line `{format_marker}` in onboarding/config.rs — \
             either the format string moved or apply_config was restructured; update \
             this test if intentional"
        )
    });

    let preceding = &src[..format_idx];

    let guard_idx = preceding.find("self.ps.custom_name.is_empty()").expect(
        "empty-custom_name guard must precede the section-format assignment. Without it, an \
         empty custom_name falls through to a section format with `self.ps.custom_name = \"\"`, \
         producing the literal section name `providers.custom.` (a TOML write to an \
         empty-named subkey).",
    );

    // The guard must turn the provider write off, not merely test the name.
    let guard_window = &preceding[guard_idx..];
    assert!(
        guard_window.contains("write_provider = false"),
        "the empty-custom_name guard must set `write_provider = false` so the custom \
         section (and api_key) are not written against an empty name."
    );
}
