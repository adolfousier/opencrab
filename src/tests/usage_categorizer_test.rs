use crate::usage::categorizer::*;

#[test]
fn test_build_prompt() {
    let sessions = vec![
        UncategorizedSession {
            id: "abc".into(),
            title: "fix login bug".into(),
        },
        UncategorizedSession {
            id: "def".into(),
            title: "add search feature".into(),
        },
    ];
    let prompt = build_classification_prompt(&sessions);
    assert!(prompt.contains("abc|fix login bug"));
    assert!(prompt.contains("def|add search feature"));
    assert!(prompt.contains("Development"));
}

#[test]
fn test_parse_response_valid() {
    let resp = "abc-123|Bug Fixes\ndef-456|Features\n";
    let result = parse_classification_response(resp);
    assert_eq!(result.len(), 2);
    assert_eq!(result[0], ("abc-123".into(), "Bug Fixes".into()));
    assert_eq!(result[1], ("def-456".into(), "Features".into()));
}

#[test]
fn test_parse_response_filters_invalid() {
    let resp = "abc|Bug Fixes\ndef|InvalidCategory\nghi|Development\n";
    let result = parse_classification_response(resp);
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].1, "Bug Fixes");
    assert_eq!(result[1].1, "Development");
}

#[test]
fn test_parse_response_handles_garbage() {
    let resp = "random garbage\n\nabc|Features\n|empty\n";
    let result = parse_classification_response(resp);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0], ("abc".into(), "Features".into()));
}
