use crate::brain::agent::service::phantom::*;

#[test]
fn english_phantom_detected() {
    assert!(has_phantom_tool_intent_no_tools(
        "Let me check the logs and fix the issue."
    ));
}

#[test]
fn russian_phantom_detected() {
    assert!(has_phantom_tool_intent_no_tools(
        "Давайте проверю логи и исправлю ошибку."
    ));
}

#[test]
fn spanish_phantom_detected() {
    assert!(has_phantom_tool_intent_no_tools(
        "Déjame revisar el archivo y voy a actualizar la configuración ¿ok?"
    ));
}

#[test]
fn portuguese_phantom_detected() {
    assert!(has_phantom_tool_intent_no_tools(
        "Vou verificar o arquivo e corrigir a configuração do irmão"
    ));
}

#[test]
fn french_phantom_detected() {
    assert!(has_phantom_tool_intent_no_tools(
        "Laissez-moi vérifier le fichierête et corriger l'erreur être"
    ));
}

#[test]
fn structured_answer_not_phantom() {
    // A table/list answer should never be flagged
    let table = "| Commit | Message |\n|---|---|\n| abc123 | fix stuff |\n";
    assert!(!has_phantom_tool_intent_no_tools(table));
}

#[test]
fn short_text_not_phantom() {
    assert!(!has_phantom_tool_intent_no_tools("ok"));
}

#[test]
fn english_completion_claim() {
    assert!(has_phantom_tool_intent(
        "I've updated the file and all changes have been applied."
    ));
}

#[test]
fn english_trailing_colon() {
    assert!(has_phantom_tool_intent(
        "Let me check the logs and verify the configuration settings:"
    ));
}

#[test]
fn english_intent_with_path() {
    assert!(has_phantom_tool_intent(
        "Let me update src/main.rs with the new configuration."
    ));
}

#[test]
fn investigative_intent_english() {
    assert!(has_investigative_intent("Let me dig into this issue."));
}

#[test]
fn stuck_in_intent_loop_english() {
    // A genuine loop: the SAME intent line repeated.
    let text = "Let me check the logs\nLet me check the logs\nLet me check the logs\n";
    assert!(is_stuck_in_intent_loop(text));
}

#[test]
fn varied_multistep_plan_is_not_a_loop() {
    // Distinct intent lines = a legitimate plan, NOT a phantom loop. This
    // is the false positive that killed planful replies (Telegram 16:13).
    let text = "Let me check the logs\nLet me verify the config\nLet me read the file\n";
    assert!(!is_stuck_in_intent_loop(text));
}

#[test]
fn not_stuck_single_intent() {
    let text = "Let me check the logs and see what happened.";
    assert!(!is_stuck_in_intent_loop(text));
}

#[test]
fn looks_truncated() {
    assert!(looks_truncated_mid_sentence(
        "This is a long response that got cut off in the middle of a wor"
    ));
}

#[test]
fn not_truncated_with_period() {
    assert!(!looks_truncated_mid_sentence(
        "This is a complete response that ends with a period."
    ));
}

// ── #463: language coverage and the language-agnostic tool-name tell ──

#[test]
fn past_tense_loaded_claim_is_phantom() {
    // "loaded" was missing from action_verbs; the live hallucination closed
    // with this exact claim and zero tool calls.
    assert!(has_phantom_tool_intent_no_tools(
        "Both loaded properly into context via load_brain_file. Ready for the next task."
    ));
}

#[test]
fn indonesian_intent_phrases_detected() {
    // Indonesian is ASCII-Latin, so char-set language detection cannot route
    // it; the intent scan checks all loaded languages at once.
    assert!(has_phantom_tool_intent_no_tools(
        "Sekarang gue jalankan tool-nya buat load file itu ke context."
    ));
    assert!(has_phantom_tool_intent_no_tools(
        "Biar gue load dua-duanya dengan benar sekarang."
    ));
}

#[test]
fn registered_tool_mention_detected_word_boundary() {
    let names = vec![
        "load_brain_file".to_string(),
        "read_file".to_string(),
        "bash".to_string(),
        "plan".to_string(),
    ];
    // Underscore names match at word boundaries, any language.
    assert!(mentions_registered_tool(
        "Both loaded properly into context via load_brain_file.",
        &names
    ));
    assert!(mentions_registered_tool(
        "Gue udah load pakai read_file which is file-level access.",
        &names
    ));
    // Substrings of larger identifiers do not match.
    assert!(!mentions_registered_tool(
        "the loader_read_filesystem module handles this",
        &names
    ));
    // Single-word tool names are ordinary prose words: never matched.
    assert!(!mentions_registered_tool(
        "run it in bash and plan accordingly",
        &names
    ));
}
