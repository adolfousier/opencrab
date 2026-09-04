//! Structural query detection and symbol-graph search (code-graph).

use crate::memory::search::*;

#[test]
fn test_detect_structural_query_calls() {
    let result = detect_structural_query("who calls process_message");
    assert_eq!(
        result,
        Some(("calls".to_string(), "process_message".to_string()))
    );

    let result = detect_structural_query("what calls validate_input");
    assert_eq!(
        result,
        Some(("calls".to_string(), "validate_input".to_string()))
    );
}

#[test]
fn test_detect_structural_query_called_by() {
    let result = detect_structural_query("what does process_message call");
    assert_eq!(
        result,
        Some(("called_by".to_string(), "process_message".to_string()))
    );
}

#[test]
fn test_detect_structural_query_implements() {
    let result = detect_structural_query("show implementations of Drawable");
    assert_eq!(
        result,
        Some(("implements".to_string(), "drawable".to_string()))
    );

    let result = detect_structural_query("who implements Serializable");
    assert_eq!(
        result,
        Some(("implements".to_string(), "serializable".to_string()))
    );
}

#[test]
fn test_detect_structural_query_defined_in() {
    let result = detect_structural_query("where is process_message defined");
    assert_eq!(
        result,
        Some(("defined_in".to_string(), "process_message".to_string()))
    );

    let result = detect_structural_query("show definition of validate_input");
    assert_eq!(
        result,
        Some(("defined_in".to_string(), "validate_input".to_string()))
    );
}

#[test]
fn test_detect_structural_query_conceptual() {
    // These should NOT match structural patterns
    let result = detect_structural_query("context compaction");
    assert_eq!(result, None);

    let result = detect_structural_query("telegram rich cards");
    assert_eq!(result, None);

    let result = detect_structural_query("how does memory search work");
    assert_eq!(result, None);
}
