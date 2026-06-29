use super::*;

#[test]
fn test_highlight_rust() {
    let code = "fn main() {\n    println!(\"Hello, world!\");\n}";
    let lines = highlight_code(code, "rust");
    assert_eq!(lines.len(), 3);
    assert!(!lines[0].spans.is_empty());
}

#[test]
fn test_highlight_python() {
    let code = "def hello():\n    print(\"Hello, world!\")";
    let lines = highlight_code(code, "python");
    assert_eq!(lines.len(), 2);
}

#[test]
fn test_highlight_javascript() {
    let code = "function hello() {\n  console.log(\"Hello\");\n}";
    let lines = highlight_code(code, "javascript");
    assert_eq!(lines.len(), 3);
}

#[test]
fn test_highlight_unknown_language() {
    let code = "some code";
    let lines = highlight_code(code, "unknown_language");
    assert_eq!(lines.len(), 1);
    // Should still render, just without syntax highlighting
}

#[test]
fn test_supported_languages() {
    let langs = supported_languages();
    assert!(!langs.is_empty());
    assert!(langs.contains(&"Rust".to_string()));
    assert!(langs.contains(&"Python".to_string()));
}

#[test]
fn test_is_language_supported() {
    assert!(is_language_supported("rust"));
    assert!(is_language_supported("Rust"));
    assert!(is_language_supported("python"));
    assert!(is_language_supported("javascript"));
    assert!(!is_language_supported("not_a_real_language"));
}

#[test]
fn test_empty_code() {
    let code = "";
    let lines = highlight_code(code, "rust");
    // Should handle empty code gracefully
    assert!(lines.is_empty() || lines.len() == 1);
}

#[test]
fn test_code_with_special_characters() {
    let code = "let x = \"Hello, 世界!\";";
    let lines = highlight_code(code, "rust");
    assert_eq!(lines.len(), 1);
}
