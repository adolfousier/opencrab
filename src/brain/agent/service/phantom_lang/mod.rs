//! Per-language phantom-detection data loaded from TOML at compile time.
//!
//! Each `.toml` file defines the phrases, verbs, and regex patterns
//! for one language. The loader embeds them into the binary via
//! `include_str!` so any TOML syntax error fails the build.
//!
//! Runtime language detection picks the right config based on
//! character-set heuristics (Cyrillic → ru, etc.).

use serde::Deserialize;
use std::sync::LazyLock;

/// Language-specific phantom detection configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct LangConfig {
    #[serde(default)]
    pub intent_phrases: Vec<String>,
    #[serde(default)]
    pub action_verbs: Vec<String>,
    #[serde(default)]
    pub line_start_re: String,
    /// Matches a brief present-continuous work announcement that ends the
    /// turn with no tool call — e.g. "Running checks now.", "Checking the
    /// logs…". These fall under the no-tools detector's length floor and
    /// aren't "I'll / let me / I'm going to" phrases, so they need their own
    /// pattern. Anchored and ending in an imminence marker (now / … / :) to
    /// avoid matching ordinary sentences that merely open with a gerund.
    #[serde(default)]
    pub work_announcement_re: String,
    #[serde(default)]
    pub completion_claims: Vec<String>,
    /// Phrases presenting a named command as ALREADY RUN, e.g. "ran",
    /// "checked with", "output above". Required by the uncalled-command check
    /// (#789): the command itself is language-neutral, but the framing that
    /// turns a proposal into a claim is not, and an English-only list let a
    /// fabrication in any other language through untouched.
    #[serde(default)]
    pub executed_framings: Vec<String>,
    #[serde(default)]
    pub gerund_re: String,
    #[serde(default)]
    pub trailing_colon_re: String,
    #[serde(default)]
    pub now_imperative_re: String,
    #[serde(default)]
    pub numbered_steps_re: String,
    #[serde(default)]
    pub past_tense_standalone_re: String,
    #[serde(default)]
    pub path_re: String,
    #[serde(default)]
    pub ext_re: String,
    #[serde(default)]
    pub backtick_code_re: String,
    /// Assertions that a visual/media result was produced or delivered
    /// ("there it is", "generated it", "the edited image", ...). Paired with
    /// `media_context_words` to catch image-generation hallucination in any
    /// language (#747) — a claim of a delivered image with no `<<IMG:>>` marker.
    #[serde(default)]
    pub media_delivery_phrases: Vec<String>,
    /// Words marking a claimed result as visual/image work ("image", "photo",
    /// "background", "brightness", ...). Paired with `media_delivery_phrases`.
    #[serde(default)]
    pub media_context_words: Vec<String>,
    /// Claims that a FILE or DOCUMENT was sent ("file sent", "attached the
    /// file", "sent the report"). Paired with `file_context_words` to catch a
    /// delivery claim in a turn that never invoked a document-sending tool
    /// (#825). Document sending is newer than image generation, so the media
    /// checks (#747) did not cover it.
    #[serde(default)]
    pub file_delivery_phrases: Vec<String>,
    /// Words marking the claimed deliverable as a file rather than a message
    /// ("file", "document", "report", ".md"). Paired with
    /// `file_delivery_phrases`.
    #[serde(default)]
    pub file_context_words: Vec<String>,
}

/// Embedded TOML content (compile-time validated).
const EN_TOML: &str = include_str!("en.toml");
const RU_TOML: &str = include_str!("ru.toml");
const ES_TOML: &str = include_str!("es.toml");
const PT_TOML: &str = include_str!("pt.toml");
const FR_TOML: &str = include_str!("fr.toml");
const ID_TOML: &str = include_str!("id.toml");

pub(crate) static LANG_EN: LazyLock<LangConfig> =
    LazyLock::new(|| toml::from_str(EN_TOML).expect("BUG: en.toml failed to parse at runtime"));
pub(crate) static LANG_RU: LazyLock<LangConfig> =
    LazyLock::new(|| toml::from_str(RU_TOML).expect("BUG: ru.toml failed to parse at runtime"));
pub(crate) static LANG_ES: LazyLock<LangConfig> =
    LazyLock::new(|| toml::from_str(ES_TOML).expect("BUG: es.toml failed to parse at runtime"));
pub(crate) static LANG_PT: LazyLock<LangConfig> =
    LazyLock::new(|| toml::from_str(PT_TOML).expect("BUG: pt.toml failed to parse at runtime"));
pub(crate) static LANG_FR: LazyLock<LangConfig> =
    LazyLock::new(|| toml::from_str(FR_TOML).expect("BUG: fr.toml failed to parse at runtime"));
pub(crate) static LANG_ID: LazyLock<LangConfig> =
    LazyLock::new(|| toml::from_str(ID_TOML).expect("BUG: id.toml failed to parse at runtime"));

/// Detect language from text content using character-set heuristics.
/// Returns a static reference to the appropriate language config.
pub fn detect_language(text: &str) -> &'static LangConfig {
    let mut cyrillic = 0u32;
    let mut latin_accent = 0u32;
    let mut total_alpha = 0u32;

    for ch in text.chars().take(500) {
        if ch.is_alphabetic() {
            total_alpha += 1;
            if ('\u{0400}'..='\u{04FF}').contains(&ch) {
                cyrillic += 1;
            } else if ('\u{00C0}'..='\u{024F}').contains(&ch) {
                latin_accent += 1;
            }
        }
    }

    if total_alpha == 0 {
        return &LANG_EN;
    }

    // Cyrillic > 20% of alpha chars → Russian
    if cyrillic * 5 > total_alpha {
        return &LANG_RU;
    }

    // For Latin-accent text, distinguish Spanish/Portuguese/French
    // by looking for language-specific characters
    if latin_accent > 0 {
        // Portuguese-specific: ã, õ, ç
        if text.contains('ã')
            || text.contains('õ')
            || text.contains('ç')
            || text.contains('Ã')
            || text.contains('Õ')
            || text.contains('Ç')
        {
            return &LANG_PT;
        }
        // Spanish-specific: ñ, ¿, ¡
        if text.contains('ñ') || text.contains('Ñ') || text.contains('¿') || text.contains('¡')
        {
            return &LANG_ES;
        }
        // If we have significant accented Latin but no PT/ES markers,
        // check for French patterns (à, â, ç, é, è, ê, ë, î, ï, ô, ù, û, ü, ÿ)
        // French is the fallback for accented Latin since it's the most
        // common accented-Latin language after Spanish/Portuguese
        if text.contains('à')
            || text.contains('â')
            || text.contains('é')
            || text.contains('è')
            || text.contains('ê')
            || text.contains('ë')
            || text.contains('î')
            || text.contains('ï')
            || text.contains('ô')
            || text.contains('û')
            || text.contains('ù')
            || text.contains('ü')
            || text.contains('ÿ')
        {
            return &LANG_FR;
        }
    }

    &LANG_EN
}

/// Every loaded language config, in detection-priority order.
///
/// The phantom-intent detectors scan intent phrases across all languages
/// at once: `detect_language` only routes Cyrillic and accented-Latin
/// text reliably, so accent-free non-English narration (e.g.
/// `"Voy a usar write_file…"`, no ñ/¿) otherwise falls through to English
/// and slips past. Intent phrases are multi-word and carry
/// language-distinctive tokens, so cross-language scanning is
/// collision-free — unlike the short single-word `action_verbs`, which
/// stay gated to the detected language. 2026-06-12.
pub fn all_langs() -> [&'static LangConfig; 6] {
    [&LANG_EN, &LANG_RU, &LANG_ES, &LANG_PT, &LANG_FR, &LANG_ID]
}
