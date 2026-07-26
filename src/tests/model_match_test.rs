//! Naming a model the way people say it (#801).
//!
//! Switching by argument required the exact slash-separated id, so the user
//! had to know a model's punctuation to name it. The words are what people
//! remember; the hyphens and dots are not.
//!
//! The risk of loose matching is guessing. A matcher that picks between
//! plausible candidates is worse than none, because the user gets a model they
//! did not ask for and has no reason to check, so refusing is tested as
//! carefully as matching.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::utils::model_match::{ModelMatch, match_model, normalize, split_provider_and_model};

fn catalogue() -> Vec<String> {
    ["mimo-v2.5-pro", "mimo-v2.5-lite", "glm-5.1", "qwen3.8-max"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

#[test]
fn spacing_and_punctuation_do_not_matter() {
    for form in [
        "mimo v2.5 pro",
        "mimo-v2.5-pro",
        "MiMo V2.5 Pro",
        "mimov25pro",
    ] {
        assert_eq!(
            match_model(form, &catalogue()),
            ModelMatch::One("mimo-v2.5-pro".to_string()),
            "form should resolve: {form}"
        );
    }
}

#[test]
fn normalize_reduces_to_alphanumerics() {
    assert_eq!(normalize("MiMo V2.5 Pro"), "mimov25pro");
    assert_eq!(normalize("mimo-v2.5-pro"), "mimov25pro");
}

#[test]
fn an_ambiguous_reference_refuses_and_lists() {
    // "mimo v2.5" prefixes both the pro and the lite. Picking either would be
    // a silent substitution.
    match match_model("mimo v2.5", &catalogue()) {
        ModelMatch::Ambiguous(hits) => assert_eq!(hits.len(), 2, "got: {hits:?}"),
        other => panic!("must refuse rather than choose: {other:?}"),
    }
}

#[test]
fn an_exact_name_beats_a_longer_one_containing_it() {
    // Tiering exists for this: an id that IS the reference must not be
    // declared ambiguous just because another id contains it.
    let cat: Vec<String> = ["gpt-5", "gpt-5-mini", "gpt-5-turbo"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        match_model("gpt-5", &cat),
        ModelMatch::One("gpt-5".to_string())
    );
}

#[test]
fn an_unknown_reference_matches_nothing() {
    assert_eq!(match_model("llama-4-scout", &catalogue()), ModelMatch::None);
}

#[test]
fn an_empty_reference_matches_nothing() {
    // Never resolve "no model given" into an arbitrary catalogue entry.
    assert_eq!(match_model("   ", &catalogue()), ModelMatch::None);
    assert_eq!(match_model("---", &catalogue()), ModelMatch::None);
}

#[test]
fn a_substring_still_resolves_when_unique() {
    // "max" appears in exactly one id, so it is a usable shorthand.
    assert_eq!(
        match_model("max", &catalogue()),
        ModelMatch::One("qwen3.8-max".to_string())
    );
}

#[test]
fn the_first_token_is_the_provider() {
    assert_eq!(
        split_provider_and_model("xiaomi mimo v2.5 pro"),
        Some(("xiaomi".to_string(), "mimo v2.5 pro".to_string()))
    );
}

#[test]
fn a_provider_with_no_model_is_not_a_pair() {
    // Reported rather than defaulted: silently choosing a model is the
    // failure this path exists to avoid.
    assert_eq!(split_provider_and_model("xiaomi"), None);
    assert_eq!(split_provider_and_model("   "), None);
}
