//! Normalise `[agent] self_improvement_provider` into the pair RSI actually
//! runs on (#1314).
//!
//! The key takes a provider name: the config section name, so
//! `[providers.custom.moonshotai]` is `"moonshotai"`. Users reasonably write
//! `"custom:moonshotai"` (the table path) or `"zhipu/glm-5.3"` (the pair they
//! see in `/models`), and a value taken verbatim then fails to build a
//! provider and every cycle dies before it starts. This module recognises
//! those spellings against the providers that are actually configured and
//! says what it corrected, so the log teaches the canonical form.
//!
//! Pure: the two predicates are injected so every branch is testable without
//! a `Config`.

/// The provider and optional model RSI should run on, after normalisation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RsiPair {
    pub provider: String,
    pub model: Option<String>,
    /// What was corrected, for a one-line warning; `None` when the value was
    /// already canonical.
    pub note: Option<String>,
}

/// Prefixes that spell the `[providers.custom.*]` table path rather than the
/// provider's name. The factory tolerates the first two; the third is how a
/// path reads to someone thinking in `<provider>/<model>` terms.
const CUSTOM_PREFIXES: [&str; 3] = ["custom:", "custom.", "custom/"];

/// Normalise `spec` (the raw `self_improvement_provider`) with `model_key`
/// (the raw `self_improvement_model`).
///
/// `is_custom` answers whether a name is a `[providers.custom.<name>]` key;
/// `is_declared` whether a name is any configured provider, custom or
/// built-in. Rules, in order:
///
/// 1. A `custom:` / `custom.` / `custom/` prefix is dropped.
/// 2. If what remains is `<a>/<b>` or `<a>:<b>` and `<a>` is a declared
///    provider, it is split: `<a>` is the provider and `<b>` the model. The
///    split happens only when the head is a known provider, because model ids
///    themselves contain both separators (`anthropic/claude-…`, `qwen:7b`).
/// 3. An explicit `self_improvement_model` wins over a model found by (2).
pub fn normalize(
    spec: &str,
    model_key: Option<&str>,
    is_custom: impl Fn(&str) -> bool,
    is_declared: impl Fn(&str) -> bool,
) -> RsiPair {
    let raw = spec.trim();
    let mut notes: Vec<String> = Vec::new();

    let mut name = raw;
    for prefix in CUSTOM_PREFIXES {
        if let Some(rest) = name.strip_prefix(prefix) {
            name = rest.trim();
            notes.push(format!(
                "dropped the '{prefix}' prefix: the provider name is the \
                 [providers.custom.<name>] section name, so write \"{name}\""
            ));
            break;
        }
    }

    let mut model_from_spec: Option<String> = None;
    if let Some(idx) = name.find(['/', ':']) {
        let (head, tail) = (name[..idx].trim(), name[idx + 1..].trim());
        if !head.is_empty() && !tail.is_empty() && (is_custom(head) || is_declared(head)) {
            notes.push(format!(
                "split \"{name}\" into provider \"{head}\" and model \"{tail}\": \
                 self_improvement_provider takes a provider name only; the model \
                 goes in self_improvement_model"
            ));
            model_from_spec = Some(tail.to_string());
            name = head;
        }
    }

    let model = match (
        model_key.map(str::trim).filter(|m| !m.is_empty()),
        model_from_spec,
    ) {
        (Some(explicit), Some(found)) if explicit != found => {
            notes.push(format!(
                "self_improvement_model = \"{explicit}\" wins over the \"{found}\" \
                 found in self_improvement_provider"
            ));
            Some(explicit.to_string())
        }
        (Some(explicit), _) => Some(explicit.to_string()),
        (None, found) => found,
    };

    RsiPair {
        provider: name.to_string(),
        model,
        note: (!notes.is_empty()).then(|| notes.join("; ")),
    }
}
