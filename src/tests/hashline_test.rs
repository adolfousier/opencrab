//! Comprehensive tests for hashline editing functionality

use crate::brain::tools::hashline::edit::HashlineEditTool;
use crate::brain::tools::hashline::hash::{format_hashline, hash_line};
use crate::brain::tools::hashline::types::{HashRef, HashlineEditInput, HashlineEditOp};
use crate::brain::tools::read::ReadTool;
use crate::brain::tools::{Tool, ToolExecutionContext};
use serde_json::json;
use tempfile::TempDir;
use uuid::Uuid;

#[test]
fn test_hash_line_deterministic() {
    let hash1 = hash_line("test content");
    let hash2 = hash_line("test content");
    assert_eq!(hash1, hash2);
    assert_eq!(hash1.len(), 2);
}

#[test]
fn test_hash_line_different_content() {
    let hash1 = hash_line("line one");
    let hash2 = hash_line("line two");
    assert_ne!(hash1, hash2);
}

#[test]
fn test_hash_line_same_content_same_hash() {
    // Pure content hashing: identical content always produces the same hash
    // regardless of position. Line-shift avalanche is handled at validation time.
    let hash1 = hash_line("same content");
    let hash2 = hash_line("same content");
    assert_eq!(hash1, hash2);
}

#[test]
fn test_hash_line_blank_lines_same_hash() {
    // Blank lines all produce the same hash (pure content, no line number)
    let hash1 = hash_line("");
    let hash2 = hash_line("");
    let hash3 = hash_line("");
    assert_eq!(hash1, hash2);
    assert_eq!(hash2, hash3);
    assert_eq!(hash1, hash3);
}

#[test]
fn test_format_hashline() {
    let formatted = format_hashline(12, "VK", "test code");
    assert_eq!(formatted, "12#VK|test code");
}

#[test]
fn test_hashref_parse_valid() {
    let href = HashRef::parse("42#AB").unwrap();
    assert_eq!(href.line, 42);
    assert_eq!(href.hash, "AB");
}

#[test]
fn test_hashref_parse_lowercase() {
    let href = HashRef::parse("10#cd").unwrap();
    assert_eq!(href.line, 10);
    assert_eq!(href.hash, "CD");
}

#[test]
fn test_hashref_parse_with_content() {
    let href = HashRef::parse("5#XY|some code here").unwrap();
    assert_eq!(href.line, 5);
    assert_eq!(href.hash, "XY");
}

#[test]
fn test_hashref_parse_missing_separator() {
    let result = HashRef::parse("42AB");
    assert!(result.is_err());
}

#[test]
fn test_hashref_parse_invalid_line() {
    let result = HashRef::parse("abc#AB");
    assert!(result.is_err());
}

#[test]
fn test_hashref_parse_zero_line() {
    let result = HashRef::parse("0#AB");
    assert!(result.is_err());
}

#[test]
fn test_hashref_parse_wrong_hash_length() {
    let result = HashRef::parse("5#ABC");
    assert!(result.is_err());
}

#[test]
fn test_hashline_edit_op_deserialize_replace() {
    let json = json!({
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
fn test_hashline_edit_op_deserialize_replace_range() {
    let json = json!({
        "op": "replace",
        "pos": "5#VK",
        "end": "8#MB",
        "lines": "replacement"
    });
    let op: HashlineEditOp = serde_json::from_value(json).unwrap();
    match op {
        HashlineEditOp::Replace { pos, end, lines } => {
            assert_eq!(pos, "5#VK");
            assert_eq!(end, Some("8#MB".to_string()));
            assert_eq!(lines, "replacement");
        }
        _ => panic!("Expected Replace"),
    }
}

#[test]
fn test_hashline_edit_op_deserialize_append() {
    let json = json!({
        "op": "append",
        "pos": "10#XY",
        "lines": "inserted line"
    });
    let op: HashlineEditOp = serde_json::from_value(json).unwrap();
    match op {
        HashlineEditOp::Append { pos, lines } => {
            assert_eq!(pos, Some("10#XY".to_string()));
            assert_eq!(lines, "inserted line");
        }
        _ => panic!("Expected Append"),
    }
}

#[test]
fn test_hashline_edit_op_deserialize_prepend() {
    let json = json!({
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
fn test_hashline_edit_input_deserialize() {
    let json = json!({
        "path": "test.rs",
        "edits": [
            {"op": "replace", "pos": "1#AB", "lines": "new"},
            {"op": "append", "pos": "5#CD", "lines": "added"}
        ]
    });
    let input: HashlineEditInput = serde_json::from_value(json).unwrap();
    assert_eq!(input.path, "test.rs");
    assert_eq!(input.edits.len(), 2);
}

#[tokio::test]
async fn test_hashline_edit_replace_single_line() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test.txt");
    std::fs::write(&file_path, "line one\nline two\nline three\n").unwrap();

    let tool = HashlineEditTool;
    let context = ToolExecutionContext::new(Uuid::new_v4())
        .with_working_directory(temp_dir.path().to_path_buf())
        .with_auto_approve(true);

    // Get hash for line 2
    let hash = hash_line("line two");
    let pos = format!("2#{}", hash);

    let input = json!({
        "path": file_path.to_str().unwrap(),
        "edits": [{
            "op": "replace",
            "pos": pos,
            "lines": "LINE TWO MODIFIED"
        }]
    });

    let result = tool.execute(input, &context).await.unwrap();
    assert!(result.success);

    let content = std::fs::read_to_string(&file_path).unwrap();
    assert!(content.contains("LINE TWO MODIFIED"));
    assert!(!content.contains("line two"));
}

#[tokio::test]
async fn test_hashline_edit_replace_range() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test.txt");
    std::fs::write(&file_path, "one\ntwo\nthree\nfour\nfive\n").unwrap();

    let tool = HashlineEditTool;
    let context = ToolExecutionContext::new(Uuid::new_v4())
        .with_working_directory(temp_dir.path().to_path_buf())
        .with_auto_approve(true);

    let hash2 = hash_line("two");
    let hash4 = hash_line("four");

    let input = json!({
        "path": file_path.to_str().unwrap(),
        "edits": [{
            "op": "replace",
            "pos": format!("2#{}", hash2),
            "end": format!("4#{}", hash4),
            "lines": "REPLACED\nRANGE"
        }]
    });

    let result = tool.execute(input, &context).await.unwrap();
    assert!(result.success);

    let content = std::fs::read_to_string(&file_path).unwrap();
    assert!(content.contains("REPLACED"));
    assert!(content.contains("RANGE"));
    assert!(!content.contains("two"));
    assert!(!content.contains("four"));
}

#[tokio::test]
async fn test_hashline_edit_append() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test.txt");
    std::fs::write(&file_path, "line one\nline two\n").unwrap();

    let tool = HashlineEditTool;
    let context = ToolExecutionContext::new(Uuid::new_v4())
        .with_working_directory(temp_dir.path().to_path_buf())
        .with_auto_approve(true);

    let hash = hash_line("line two");

    let input = json!({
        "path": file_path.to_str().unwrap(),
        "edits": [{
            "op": "append",
            "pos": format!("2#{}", hash),
            "lines": "appended line"
        }]
    });

    let result = tool.execute(input, &context).await.unwrap();
    assert!(result.success);

    let content = std::fs::read_to_string(&file_path).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[2], "appended line");
}

#[tokio::test]
async fn test_hashline_edit_prepend() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test.txt");
    std::fs::write(&file_path, "line one\nline two\n").unwrap();

    let tool = HashlineEditTool;
    let context = ToolExecutionContext::new(Uuid::new_v4())
        .with_working_directory(temp_dir.path().to_path_buf())
        .with_auto_approve(true);

    let hash = hash_line("line one");

    let input = json!({
        "path": file_path.to_str().unwrap(),
        "edits": [{
            "op": "prepend",
            "pos": format!("1#{}", hash),
            "lines": "prepended line"
        }]
    });

    let result = tool.execute(input, &context).await.unwrap();
    assert!(result.success);

    let content = std::fs::read_to_string(&file_path).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0], "prepended line");
}

#[tokio::test]
async fn test_hashline_edit_hash_mismatch() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test.txt");
    std::fs::write(&file_path, "line one\nline two\n").unwrap();

    let tool = HashlineEditTool;
    let context = ToolExecutionContext::new(Uuid::new_v4())
        .with_working_directory(temp_dir.path().to_path_buf())
        .with_auto_approve(true);

    // Use wrong hash
    let input = json!({
        "path": file_path.to_str().unwrap(),
        "edits": [{
            "op": "replace",
            "pos": "1#ZZ",
            "lines": "new content"
        }]
    });

    let result = tool.execute(input, &context).await.unwrap();
    assert!(!result.success);
    assert!(result.error.unwrap().contains("Hash mismatch"));
}

#[tokio::test]
async fn test_hashline_edit_overlapping_ranges() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test.txt");
    std::fs::write(&file_path, "one\ntwo\nthree\nfour\nfive\n").unwrap();

    let tool = HashlineEditTool;
    let context = ToolExecutionContext::new(Uuid::new_v4())
        .with_working_directory(temp_dir.path().to_path_buf())
        .with_auto_approve(true);

    let hash1 = hash_line("one");
    let hash3 = hash_line("three");
    let hash5 = hash_line("five");

    // Overlapping ranges: 1-3 and 3-5
    let input = json!({
        "path": file_path.to_str().unwrap(),
        "edits": [
            {
                "op": "replace",
                "pos": format!("1#{}", hash1),
                "end": format!("3#{}", hash3),
                "lines": "first"
            },
            {
                "op": "replace",
                "pos": format!("3#{}", hash3),
                "end": format!("5#{}", hash5),
                "lines": "second"
            }
        ]
    });

    let result = tool.execute(input, &context).await.unwrap();
    assert!(!result.success);
    assert!(result.error.unwrap().contains("Overlapping"));
}

#[tokio::test]
async fn test_hashline_edit_multiple_edits() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test.txt");
    std::fs::write(&file_path, "one\ntwo\nthree\nfour\n").unwrap();

    let tool = HashlineEditTool;
    let context = ToolExecutionContext::new(Uuid::new_v4())
        .with_working_directory(temp_dir.path().to_path_buf())
        .with_auto_approve(true);

    let hash1 = hash_line("one");
    let hash3 = hash_line("three");

    let input = json!({
        "path": file_path.to_str().unwrap(),
        "edits": [
            {
                "op": "replace",
                "pos": format!("1#{}", hash1),
                "lines": "ONE"
            },
            {
                "op": "replace",
                "pos": format!("3#{}", hash3),
                "lines": "THREE"
            }
        ]
    });

    let result = tool.execute(input, &context).await.unwrap();
    assert!(result.success);

    let content = std::fs::read_to_string(&file_path).unwrap();
    assert!(content.contains("ONE"));
    assert!(content.contains("THREE"));
}

#[tokio::test]
async fn test_hashline_edit_strip_prefix_autocorrect() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test.txt");
    std::fs::write(&file_path, "line one\nline two\n").unwrap();

    let tool = HashlineEditTool;
    let context = ToolExecutionContext::new(Uuid::new_v4())
        .with_working_directory(temp_dir.path().to_path_buf())
        .with_auto_approve(true);

    let hash = hash_line("line one");

    // Include hashline prefix in the replacement (model mistake)
    let input = json!({
        "path": file_path.to_str().unwrap(),
        "edits": [{
            "op": "replace",
            "pos": format!("1#{}", hash),
            "lines": format!("1#{}|NEW CONTENT", hash)
        }]
    });

    let result = tool.execute(input, &context).await.unwrap();
    assert!(result.success);

    let content = std::fs::read_to_string(&file_path).unwrap();
    // Should strip the prefix and only contain "NEW CONTENT"
    assert!(content.contains("NEW CONTENT"));
    assert!(!content.contains(format!("1#{}", hash).as_str()));
}

#[tokio::test]
async fn test_read_file_hashline_mode() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test.txt");
    std::fs::write(&file_path, "line one\nline two\nline three\n").unwrap();

    let tool = ReadTool;
    let context = ToolExecutionContext::new(Uuid::new_v4())
        .with_working_directory(temp_dir.path().to_path_buf());

    let input = json!({
        "path": file_path.to_str().unwrap(),
        "hashline": true
    });

    let result = tool.execute(input, &context).await.unwrap();
    assert!(result.success);

    let output = result.output;
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines.len(), 3);

    // Check format: LINE#ID|content
    assert!(lines[0].starts_with("1#"));
    assert!(lines[0].contains("|line one"));
    assert!(lines[1].starts_with("2#"));
    assert!(lines[1].contains("|line two"));
    assert!(lines[2].starts_with("3#"));
    assert!(lines[2].contains("|line three"));
}

#[tokio::test]
async fn test_read_file_hashline_default_false() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test.txt");
    std::fs::write(&file_path, "line one\nline two\n").unwrap();

    let tool = ReadTool;
    let context = ToolExecutionContext::new(Uuid::new_v4())
        .with_working_directory(temp_dir.path().to_path_buf());

    let input = json!({
        "path": file_path.to_str().unwrap()
    });

    let result = tool.execute(input, &context).await.unwrap();
    assert!(result.success);

    let output = result.output;
    // Should NOT contain hashline format
    assert!(!output.contains("#"));
    assert!(!output.contains("|"));
    assert_eq!(output.trim(), "line one\nline two");
}

#[tokio::test]
async fn test_read_file_hashline_with_start_line() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test.txt");
    std::fs::write(&file_path, "one\ntwo\nthree\nfour\nfive\n").unwrap();

    let tool = ReadTool;
    let context = ToolExecutionContext::new(Uuid::new_v4())
        .with_working_directory(temp_dir.path().to_path_buf());

    let input = json!({
        "path": file_path.to_str().unwrap(),
        "start_line": 2,
        "line_count": 2,
        "hashline": true
    });

    let result = tool.execute(input, &context).await.unwrap();
    assert!(result.success);

    let output = result.output;
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines.len(), 2);

    // Should start at line 3 (1-indexed, since start_line is 2 which is 0-indexed)
    assert!(lines[0].starts_with("3#"));
    assert!(lines[0].contains("|three"));
    assert!(lines[1].starts_with("4#"));
    assert!(lines[1].contains("|four"));
}
