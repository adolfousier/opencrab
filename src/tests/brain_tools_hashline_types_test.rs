use super::*;

#[test]
fn test_parse_hashref_valid() {
    let hr = HashRef::parse("12#VK").unwrap();
    assert_eq!(hr.hash, "VK");
}

#[test]
fn test_parse_hashref_single_digit() {
    let hr = HashRef::parse("1#ZP").unwrap();
    assert_eq!(hr.hash, "ZP");
}

#[test]
fn test_parse_hashref_large_line() {
    let hr = HashRef::parse("1234#AB").unwrap();
    assert_eq!(hr.hash, "AB");
}

#[test]
fn test_parse_hashref_with_pipe_content() {
    // Model might include the content after the pipe
    let hr = HashRef::parse("5#XY|some code here").unwrap();
    assert_eq!(hr.hash, "XY");
}

#[test]
fn test_parse_hashref_lowercase_uppercased() {
    let hr = HashRef::parse("3#vk").unwrap();
    assert_eq!(hr.hash, "VK");
}

#[test]
fn test_parse_hashref_missing_separator() {
    assert!(HashRef::parse("12VK").is_err());
}

#[test]
fn test_parse_hashref_invalid_line_ignored() {
    // Legacy format: line number is ignored, only hash matters
    let hr = HashRef::parse("abc#VK").unwrap();
    assert_eq!(hr.hash, "VK");
}

#[test]
fn test_parse_hashref_zero_line_ignored() {
    // Legacy format: line number is ignored, only hash matters
    let hr = HashRef::parse("0#VK").unwrap();
    assert_eq!(hr.hash, "VK");
}

#[test]
fn test_parse_hashref_wrong_hash_length() {
    assert!(HashRef::parse("5#V").is_err());
    assert!(HashRef::parse("5#VKA").is_err());
}

#[test]
fn test_deserialize_replace_op() {
    let json = serde_json::json!({
        "op": "replace",
        "pos": "5#VK",
        "lines": "new content"
    });
    let op: HashlineEditOp = serde_json::from_value(json).unwrap();
    match op {
        HashlineEditOp::Replace { pos, end, lines } => {
            assert_eq!(pos, "5#VK");
            assert!(end.is_none());
            assert_eq!(lines, "new content");
        }
        _ => panic!("Expected Replace"),
    }
}

#[test]
fn test_deserialize_replace_range() {
    let json = serde_json::json!({
        "op": "replace",
        "pos": "5#VK",
        "end": "8#MB",
        "lines": "replacement"
    });
    let op: HashlineEditOp = serde_json::from_value(json).unwrap();
    match op {
        HashlineEditOp::Replace { pos, end, lines } => {
            assert_eq!(pos, "5#VK");
            assert_eq!(end.unwrap(), "8#MB");
            assert_eq!(lines, "replacement");
        }
        _ => panic!("Expected Replace"),
    }
}

#[test]
fn test_deserialize_append() {
    let json = serde_json::json!({
        "op": "append",
        "pos": "10#XY",
        "lines": "inserted line"
    });
    let op: HashlineEditOp = serde_json::from_value(json).unwrap();
    match op {
        HashlineEditOp::Append { pos, lines } => {
            assert_eq!(pos.unwrap(), "10#XY");
            assert_eq!(lines, "inserted line");
        }
        _ => panic!("Expected Append"),
    }
}

#[test]
fn test_deserialize_prepend() {
    let json = serde_json::json!({
        "op": "prepend",
        "lines": "header line"
    });
    let op: HashlineEditOp = serde_json::from_value(json).unwrap();
    match op {
        HashlineEditOp::Prepend { pos, lines } => {
            assert!(pos.is_none());
            assert_eq!(lines, "header line");
        }
        _ => panic!("Expected Prepend"),
    }
}

#[test]
fn test_deserialize_full_input() {
    let json = serde_json::json!({
        "path": "src/main.rs",
        "edits": [
            { "op": "replace", "pos": "1#VK", "lines": "new line 1" },
            { "op": "append", "pos": "5#MB", "lines": "inserted" }
        ]
    });
    let input: HashlineEditInput = serde_json::from_value(json).unwrap();
    assert_eq!(input.path, "src/main.rs");
    assert_eq!(input.edits.len(), 2);
}
