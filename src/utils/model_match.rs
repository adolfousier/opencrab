//! Resolve a loosely-typed model reference against a provider's catalogue
//! (#801).
//!
//! Switching by argument used to require the exact slash-separated id, so the
//! user had to know a model's punctuation to name it: `xiaomi/mimo-v2.5-pro`
//! worked, `xiaomi mimo v2.5 pro` did not. The words are the part people
//! remember; the hyphens and dots are not.
//!
//! Normalizing both sides to alphanumerics makes those forms equivalent
//! without any model call, so the loose form is as programmatic and as
//! predictable as the exact one.
//!
//! Refusing is a first-class outcome here. Loose matching that guesses between
//! candidates is worse than no loose matching at all: the user gets a model
//! they did not ask for and has no reason to check.

/// Reduce a reference to comparable form: lowercase, alphanumerics only.
///
/// `"MiMo V2.5 Pro"`, `"mimo-v2.5-pro"` and `"mimo v2.5 pro"` all become
/// `"mimov25pro"`.
pub fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

/// The outcome of matching a reference against a catalogue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelMatch {
    /// Exactly one model matched; here is its real id.
    One(String),
    /// Several matched. Carries them so the caller can show the choice
    /// instead of picking one.
    Ambiguous(Vec<String>),
    /// Nothing matched.
    None,
}

/// Find the model in `catalogue` that `reference` names.
///
/// Tried in order, stopping at the first tier that yields any hit, so an exact
/// name is never beaten by something that merely contains it:
///
/// 1. exact match on the normalized form
/// 2. a catalogue entry that starts with it
/// 3. a catalogue entry that contains it
///
/// The tiers matter for real catalogues: `gpt-5` would otherwise be ambiguous
/// against `gpt-5-mini` and `gpt-5-turbo` despite naming a real model exactly.
pub fn match_model(reference: &str, catalogue: &[String]) -> ModelMatch {
    let needle = normalize(reference);
    if needle.is_empty() {
        return ModelMatch::None;
    }

    let normalized: Vec<(String, &String)> = catalogue
        .iter()
        .map(|m| (normalize(m), m))
        .filter(|(n, _)| !n.is_empty())
        .collect();

    for tier in [
        // Exact first: an id that IS the reference always wins.
        |n: &str, needle: &str| n == needle,
        |n: &str, needle: &str| n.starts_with(needle),
        |n: &str, needle: &str| n.contains(needle),
    ] {
        let hits: Vec<String> = normalized
            .iter()
            .filter(|(n, _)| tier(n, &needle))
            .map(|(_, original)| (*original).clone())
            .collect();
        match hits.len() {
            0 => continue,
            1 => return ModelMatch::One(hits.into_iter().next().unwrap_or_default()),
            _ => return ModelMatch::Ambiguous(hits),
        }
    }
    ModelMatch::None
}

/// Split a loose argument into a provider reference and a model reference.
///
/// The provider is the first token, because that is the order the exact form
/// already uses (`provider/model`) and the order people say it. Returns `None`
/// when there is no model part, which the caller reports rather than guessing
/// a default: silently choosing a model is the failure this whole path exists
/// to avoid.
pub fn split_provider_and_model(arg: &str) -> Option<(String, String)> {
    let mut tokens = arg.split_whitespace();
    let provider = tokens.next()?.to_string();
    let model: Vec<&str> = tokens.collect();
    if model.is_empty() {
        return None;
    }
    Some((provider, model.join(" ")))
}

/// Find the provider in `known` that `reference` names.
///
/// Same tiering as models, over normalized names, so `"Google Gemini"`,
/// `"google-gemini"` and `"googlegemini"` are one thing.
pub fn match_provider(reference: &str, known: &[String]) -> ModelMatch {
    match_model(reference, known)
}
