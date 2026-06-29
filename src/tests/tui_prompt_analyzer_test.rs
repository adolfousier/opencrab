use super::*;

#[test]
fn test_plan_detection() {
    let analyzer = PromptAnalyzer::new();

    let prompt = "make a plan for implementing JWT authentication";
    let result = analyzer.analyze_and_transform(prompt);
    assert!(result.contains("CRITICAL"));
    assert!(result.contains("`plan` tool"));
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
