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
    let config = detect_language("Laissez-moi vérifier le fichierête et corriger l'erreur être");
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
