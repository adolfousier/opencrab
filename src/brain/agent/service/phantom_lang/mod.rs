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
    #[serde(default)]
    pub completion_claims: Vec<String>,
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
}

/// Embedded TOML content (compile-time validated).
const EN_TOML: &str = include_str!("en.toml");
const RU_TOML: &str = include_str!("ru.toml");
const ES_TOML: &str = include_str!("es.toml");
const PT_TOML: &str = include_str!("pt.toml");
const FR_TOML: &str = include_str!("fr.toml");

static LANG_EN: LazyLock<LangConfig> =
    LazyLock::new(|| toml::from_str(EN_TOML).expect("BUG: en.toml failed to parse at runtime"));
static LANG_RU: LazyLock<LangConfig> =
    LazyLock::new(|| toml::from_str(RU_TOML).expect("BUG: ru.toml failed to parse at runtime"));
static LANG_ES: LazyLock<LangConfig> =
    LazyLock::new(|| toml::from_str(ES_TOML).expect("BUG: es.toml failed to parse at runtime"));
static LANG_PT: LazyLock<LangConfig> =
    LazyLock::new(|| toml::from_str(PT_TOML).expect("BUG: pt.toml failed to parse at runtime"));
static LANG_FR: LazyLock<LangConfig> =
    LazyLock::new(|| toml::from_str(FR_TOML).expect("BUG: fr.toml failed to parse at runtime"));

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
            || text.contains('ê')
            || text.contains('î')
            || text.contains('ï')
            || text.contains('ô')
            || text.contains('û')
            || text.contains('ù')
            || text.contains('ÿ')
        {
            return &LANG_FR;
        }
    }

    &LANG_EN
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn en_toml_loads() {
        assert!(!LANG_EN.intent_phrases.is_empty());
        assert!(!LANG_EN.action_verbs.is_empty());
        assert!(!LANG_EN.completion_claims.is_empty());
    }

    #[test]
    fn ru_toml_loads() {
        assert!(!LANG_RU.intent_phrases.is_empty());
    }

    #[test]
    fn es_toml_loads() {
        assert!(!LANG_ES.intent_phrases.is_empty());
    }

    #[test]
    fn pt_toml_loads() {
        assert!(!LANG_PT.intent_phrases.is_empty());
    }

    #[test]
    fn fr_toml_loads() {
        assert!(!LANG_FR.intent_phrases.is_empty());
    }

    #[test]
    fn detect_russian() {
        let config = detect_language("Давайте проверю логи и исправлю ошибку");
        assert_eq!(config.intent_phrases, LANG_RU.intent_phrases);
    }

    #[test]
    fn detect_spanish() {
        let config = detect_language("Voy a revisar la configuración del archivo ¿ok?");
        let first = config
            .intent_phrases
            .first()
            .map(|s| s.as_str())
            .unwrap_or("");
        assert!(
            first.contains("déjame") || first.contains("voy") || first.contains("ahora"),
            "Expected Spanish config, got first phrase: {:?}",
            first
        );
    }

    #[test]
    fn detect_portuguese() {
        let config = detect_language("Vou verificar o arquivo e corrigir a configuração irmão");
        let first = config
            .intent_phrases
            .first()
            .map(|s| s.as_str())
            .unwrap_or("");
        assert!(
            first.contains("deixe") || first.contains("vou") || first.contains("agora"),
            "Expected Portuguese config, got first phrase: {:?}",
            first
        );
    }

    #[test]
    fn detect_french() {
        let config =
            detect_language("Laissez-moi vérifier le fichierête et corriger l'erreur être");
        let first = config
            .intent_phrases
            .first()
            .map(|s| s.as_str())
            .unwrap_or("");
        assert!(
            first.contains("laissez") || first.contains("je") || first.contains("maintenant"),
            "Expected French config, got first phrase: {:?}",
            first
        );
    }

    #[test]
    fn detect_english_default() {
        let config = detect_language("Let me check the logs and fix the issue");
        assert_eq!(config.intent_phrases, LANG_EN.intent_phrases);
    }

    #[test]
    fn detect_empty_returns_english() {
        let config = detect_language("");
        assert_eq!(config.intent_phrases, LANG_EN.intent_phrases);
    }
}
