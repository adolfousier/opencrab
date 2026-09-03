//! `*_provider` key normalisation (#1314, #1316): the `custom:` table path is
//! not part of the provider name, a `<provider>/<model>` pair is split only
//! when the head is a provider the user actually configured, and the note
//! names the key it corrected. Fixtures are synthetic; no keys or user
//! identifiers.

use crate::brain::provider_spec::{ProviderKey, ProviderPair, normalize};

const RSI: ProviderKey = ProviderKey::SELF_IMPROVEMENT;

fn custom(name: &str) -> bool {
    matches!(name, "glm-53-max" | "moonshotai")
}

fn declared(name: &str) -> bool {
    custom(name) || matches!(name, "zhipu" | "minimax")
}

#[test]
fn a_bare_provider_name_is_untouched_and_unnoted() {
    let p = normalize(RSI, "moonshotai", None, custom, declared);
    assert_eq!(
        p,
        ProviderPair {
            provider: "moonshotai".into(),
            model: None,
            note: None
        }
    );
}

#[test]
fn the_custom_prefix_is_dropped_in_every_spelling() {
    for spec in [
        "custom:glm-53-max",
        "custom.glm-53-max",
        "custom/glm-53-max",
        " custom:glm-53-max ",
    ] {
        let p = normalize(RSI, spec, None, custom, declared);
        assert_eq!(p.provider, "glm-53-max", "{spec}");
        assert_eq!(p.model, None, "{spec}");
        let note = p.note.expect(spec);
        assert!(note.contains("write \"glm-53-max\""), "{note}");
    }
}

#[test]
fn a_declared_provider_slash_model_is_split() {
    for spec in ["zhipu/glm-5.3", "zhipu:glm-5.3"] {
        let p = normalize(RSI, spec, None, custom, declared);
        assert_eq!(p.provider, "zhipu", "{spec}");
        assert_eq!(p.model.as_deref(), Some("glm-5.3"), "{spec}");
        assert!(p.note.unwrap().contains("self_improvement_model"), "{spec}");
    }
}

#[test]
fn a_custom_prefix_and_a_model_are_both_corrected() {
    let p = normalize(RSI, "custom:moonshotai/kimi-k2", None, custom, declared);
    assert_eq!(p.provider, "moonshotai");
    assert_eq!(p.model.as_deref(), Some("kimi-k2"));
    let note = p.note.unwrap();
    assert!(note.contains("dropped the 'custom:' prefix"), "{note}");
    assert!(note.contains("split"), "{note}");
}

#[test]
fn an_unknown_head_is_not_split_because_model_ids_carry_separators() {
    // "anthropic" is not declared in this fixture: an openrouter-style id
    // must not be torn apart into a phantom provider.
    let p = normalize(RSI, "anthropic/claude-opus-5", None, custom, declared);
    assert_eq!(p.provider, "anthropic/claude-opus-5");
    assert_eq!(p.model, None);
    assert_eq!(p.note, None);
}

#[test]
fn the_explicit_model_key_wins_over_a_model_in_the_spec() {
    let p = normalize(RSI, "zhipu/glm-5.3", Some("glm-5.3-air"), custom, declared);
    assert_eq!(p.provider, "zhipu");
    assert_eq!(p.model.as_deref(), Some("glm-5.3-air"));
    assert!(
        p.note.unwrap().contains("wins over"),
        "explicit key must be named"
    );

    // Same model in both places is not a disagreement worth a note line.
    let same = normalize(RSI, "zhipu/glm-5.3", Some("glm-5.3"), custom, declared);
    assert_eq!(same.model.as_deref(), Some("glm-5.3"));
    assert!(!same.note.unwrap().contains("wins over"));
}

#[test]
fn an_empty_model_key_counts_as_unset() {
    let p = normalize(RSI, "minimax", Some("  "), custom, declared);
    assert_eq!(p.model, None);
    assert_eq!(p.note, None);
}

#[test]
fn the_note_names_the_key_it_corrected() {
    let p = normalize(
        ProviderKey::SUBAGENT,
        "zhipu/glm-5.3",
        Some("glm-5.3-air"),
        custom,
        declared,
    );
    let note = p.note.unwrap();
    assert!(
        note.contains("subagent_provider takes a provider name only"),
        "{note}"
    );
    assert!(
        note.contains("subagent_model = \"glm-5.3-air\" wins"),
        "{note}"
    );
    assert!(!note.contains("self_improvement"), "{note}");
}
