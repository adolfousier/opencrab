//! Character-set language detection and the all-languages scan order.

use super::config::{LANG_EN, LANG_ES, LANG_FR, LANG_ID, LANG_PT, LANG_RU, LangConfig};

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
