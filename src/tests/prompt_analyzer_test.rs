use crate::utils::prompt_analyzer::*;

#[test]
fn test_plan_detection() {
    let analyzer = PromptAnalyzer::new();

    let prompt = "make a plan for implementing JWT authentication";
    let result = analyzer.analyze_and_transform(prompt);
    assert!(result.contains("CRITICAL"));
    assert!(result.contains("`plan` tool"));
}

#[test]
fn test_plan_hint_teaches_live_ops_only() {
    let analyzer = PromptAnalyzer::new();

    let prompt = "make a plan for the migration";
    let result = analyzer.analyze_and_transform(prompt);
    // Live tool contract: init -> add_task -> start (complete listed as valid).
    assert!(result.contains("operation='init'"));
    assert!(result.contains("operation='add_task'"));
    assert!(result.contains("operation='start'"));
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
    assert!(result.contains("`plan` tool"));
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
    assert!(result.contains("`plan` tool"));
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
