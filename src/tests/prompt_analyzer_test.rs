use crate::utils::prompt_analyzer::*;

#[test]
fn test_plan_detection() {
    let analyzer = PromptAnalyzer::new();

    let prompt = "make a plan for implementing JWT authentication";
    let result = analyzer.analyze_and_transform(prompt);
    assert!(result.contains("CRITICAL"));
    assert!(result.contains("design track"));
    assert!(analyzer.plan_intent(prompt));
}

#[test]
fn test_plan_hint_teaches_design_track() {
    let analyzer = PromptAnalyzer::new();

    let prompt = "make a plan for the migration";
    let result = analyzer.analyze_and_transform(prompt);
    // Design track: init mode=design, write the SESSION PLAN .md, wait
    // for user Approve. Never start, never edit project files.
    assert!(result.contains("mode='design'"));
    assert!(result.contains("SESSION PLAN"));
    assert!(result.contains("APPROVE"));
    assert!(result.contains("Do NOT"));
    // Dead ops must never be taught as callable operations.
    assert!(!result.contains("operation='create'"));
    assert!(!result.contains("operation='finalize'"));
    // The hint explicitly rejects them by name.
    assert!(result.contains("NO 'create'"));
    assert!(result.contains("NO 'finalize'"));
}

#[test]
fn test_read_file_detection() {
    let analyzer = PromptAnalyzer::new();

    let prompt = "read the file src/main.rs and explain it";
    let result = analyzer.analyze_and_transform(prompt);
    assert!(result.contains("TOOL HINT"));
    assert!(result.contains("`read_file` tool"));
}

#[test]
fn test_search_detection() {
    let analyzer = PromptAnalyzer::new();

    let prompt = "search for the function getUserData";
    let result = analyzer.analyze_and_transform(prompt);
    assert!(result.contains("TOOL HINT"));
    assert!(result.contains("`grep` tool"));
}

#[test]
fn test_multiple_detections() {
    let analyzer = PromptAnalyzer::new();

    let prompt = "read file config.toml and make a plan to update it";
    let result = analyzer.analyze_and_transform(prompt);
    assert!(result.contains("mode='design'"));
    assert!(result.contains("`read_file` tool"));
}

#[test]
fn test_no_detection() {
    let analyzer = PromptAnalyzer::new();

    let prompt = "explain how to use rust";
    let result = analyzer.analyze_and_transform(prompt);
    assert_eq!(result, prompt);
}

#[test]
fn test_case_insensitive() {
    let analyzer = PromptAnalyzer::new();

    let prompt = "MAKE A PLAN for this feature";
    let result = analyzer.analyze_and_transform(prompt);
    assert!(result.contains("CRITICAL"));
    assert!(result.contains("mode='design'"));
}

#[test]
fn test_web_search_detection() {
    let analyzer = PromptAnalyzer::new();

    let prompt = "search the web for rust async best practices";
    let result = analyzer.analyze_and_transform(prompt);
    assert!(result.contains("TOOL HINT"));
    assert!(result.contains("`web_search` tool"));
}

#[test]
fn test_bash_detection() {
    let analyzer = PromptAnalyzer::new();

    let prompt = "run command cargo build";
    let result = analyzer.analyze_and_transform(prompt);
    assert!(result.contains("TOOL HINT"));
    assert!(result.contains("`bash` tool"));
}

#[test]
fn test_hints_for_returns_hints_only() {
    let analyzer = PromptAnalyzer::new();

    // Hints never echo the prompt back: callers append them to the LLM
    // agent string while the display path keeps the original text.
    let prompt = "make a plan for the rollout";
    let hints = analyzer.hints_for(prompt).expect("plan keywords must hint");
    assert!(!hints.contains(prompt));
    assert!(hints.starts_with("\n\n"));
    assert_eq!(
        analyzer.analyze_and_transform(prompt),
        format!("{prompt}{hints}")
    );
}

#[test]
fn test_hints_for_none_without_keywords() {
    let analyzer = PromptAnalyzer::new();
    assert!(analyzer.hints_for("explain how to use rust").is_none());
}

#[test]
fn test_shared_instance_matches_new() {
    let prompt = "make a plan for the rollout";
    assert_eq!(
        PromptAnalyzer::shared().hints_for(prompt),
        PromptAnalyzer::new().hints_for(prompt)
    );
}

#[test]
fn test_natural_chat_accepts_plain_text() {
    assert!(is_natural_chat("make a plan for the rollout"));
    assert!(is_natural_chat("  find the config loader"));
}

#[test]
fn test_natural_chat_rejects_slash_commands() {
    assert!(!is_natural_chat("/drop_release"));
    assert!(!is_natural_chat("  /plan something"));
}

#[test]
fn test_natural_chat_rejects_system_triggers() {
    assert!(!is_natural_chat(
        "[SYSTEM: Compact context now. Summarize this conversation for continuity.]"
    ));
    assert!(!is_natural_chat(
        "[System: You just upgraded from v1 to v2.]"
    ));
}

// === Per-language tests ===

#[test]
fn test_russian_plan_detection() {
    let analyzer = PromptAnalyzer::new();
    let prompt = "составь план для миграции";
    let result = analyzer.analyze_and_transform(prompt);
    assert!(result.contains("CRITICAL"));
    assert!(result.contains("design track"));
    assert!(analyzer.plan_intent(prompt));
}

#[test]
fn test_portuguese_bash_detection() {
    let analyzer = PromptAnalyzer::new();
    let prompt = "executa comando cargo build";
    let result = analyzer.analyze_and_transform(prompt);
    assert!(result.contains("TOOL HINT"));
    assert!(result.contains("`bash` tool"));
}

#[test]
fn test_spanish_read_file_detection() {
    let analyzer = PromptAnalyzer::new();
    let prompt = "lee el archivo src/main.rs";
    let result = analyzer.analyze_and_transform(prompt);
    assert!(result.contains("TOOL HINT"));
    assert!(result.contains("`read_file` tool"));
}

#[test]
fn test_french_web_search_detection() {
    let analyzer = PromptAnalyzer::new();
    let prompt = "cherche sur internet rust async";
    let result = analyzer.analyze_and_transform(prompt);
    assert!(result.contains("TOOL HINT"));
    assert!(result.contains("`web_search` tool"));
}

#[test]
fn test_indonesian_edit_file_detection() {
    let analyzer = PromptAnalyzer::new();
    let prompt = "edit file src/config.toml";
    let result = analyzer.analyze_and_transform(prompt);
    assert!(result.contains("TOOL HINT"));
    assert!(result.contains("`edit_file` tool"));
}

// === Case-insensitivity tests ===

#[test]
fn test_cyrillic_case_insensitive() {
    let analyzer = PromptAnalyzer::new();
    // ПЛАН in all caps should still trigger plan intent
    let prompt = "ПЛАН для миграции";
    assert!(analyzer.plan_intent(prompt));
}

#[test]
fn test_accented_case_insensitive() {
    let analyzer = PromptAnalyzer::new();
    // Portuguese with accented chars should trigger
    let prompt = "FAZ UM PLANO para a migração";
    let result = analyzer.analyze_and_transform(prompt);
    assert!(result.contains("CRITICAL"));
    assert!(result.contains("design track"));
}

// === Negative tests (single-word gating) ===

#[test]
fn test_bare_word_does_not_trigger() {
    let analyzer = PromptAnalyzer::new();
    // "search" alone in an unrelated sentence should NOT trigger search intent
    // because it's a single-word keyword and the sentence doesn't match any multi-word phrase
    // Note: "search for" is a multi-word phrase, so we avoid that
    let prompt = "I search meaning in life";
    let result = analyzer.analyze_and_transform(prompt);
    // Should not contain search tool hint
    assert!(!result.contains("`grep` tool"));
}

#[test]
fn test_multi_word_phrase_still_triggers() {
    let analyzer = PromptAnalyzer::new();
    // "search the web" is a multi-word phrase and should trigger even with "search" in it
    let prompt = "search the web for rust tutorials";
    let result = analyzer.analyze_and_transform(prompt);
    assert!(result.contains("`web_search` tool"));
}

#[test]
fn test_language_detection_cyrillic() {
    // Verify language detection picks Russian for Cyrillic text
    let lang = detect_language("Привет мир");
    assert!(std::ptr::eq(lang, &*LANG_RU));
}

#[test]
fn test_language_detection_portuguese() {
    // Verify language detection picks Portuguese for ã/õ/ç
    let lang = detect_language("planejamento com ã");
    assert!(std::ptr::eq(lang, &*LANG_PT));
}

#[test]
fn test_language_detection_spanish() {
    // Verify language detection picks Spanish for ñ/¿/¡
    let lang = detect_language("¿cómo estás?");
    assert!(std::ptr::eq(lang, &*LANG_ES));
}

#[test]
fn test_language_detection_french() {
    // Verify language detection picks French for à/â/é
    let lang = detect_language("résumé à la carte");
    assert!(std::ptr::eq(lang, &*LANG_FR));
}

#[test]
fn test_language_detection_english_default() {
    // Verify language detection defaults to English for plain ASCII
    let lang = detect_language("hello world");
    assert!(std::ptr::eq(lang, &*LANG_EN));
}
