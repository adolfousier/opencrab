//! Types for the hashline edit tool.

use serde::{Deserialize, Serialize};

/// A reference to a specific line by its content hash.
///
/// Format: `ID` or `#ID` where ID is the 2-char hash.
/// Example: `VK` or `#VK`
///
/// Note: Line numbers are NOT included in the ref to avoid the "avalanche" problem
/// where inserting/deleting lines invalidates all subsequent refs. The tool looks
/// up the line by hash alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HashRef {
    /// 2-character content hash
    pub hash: String,
}

impl HashRef {
    /// Parse a hash reference string.
    ///
    /// Accepts formats:
    /// - `"VK"` → hash "VK"
    /// - `"#VK"` → hash "VK"
    /// - `"VK|some content"` → hash "VK" (strips content after pipe)
    /// - `"12#VK"` → hash "VK" (legacy format, line number ignored)
    pub fn parse(s: &str) -> Result<Self, String> {
        let s = s.trim();

        // Strip leading '#' if present
        let s = if let Some(stripped) = s.strip_prefix('#') {
            stripped
        } else {
            s
        };

        // Strip trailing '|' and anything after it (model might include content)
        let hash_str = if let Some(pipe_pos) = s.find('|') {
            &s[..pipe_pos]
        } else {
            s
        };

        // Handle legacy format: LINE#HASH (ignore the line number)
        let hash_str = if let Some(hash_pos) = hash_str.find('#') {
            // Legacy format like "12#VK" - extract just the hash part
            &hash_str[hash_pos + 1..]
        } else {
            hash_str
        };

        if hash_str.len() != 2 {
            return Err(format!(
                "Invalid hash ref '{}': hash must be exactly 2 characters (got '{}')",
                s, hash_str
            ));
        }

        Ok(HashRef {
            hash: hash_str.to_uppercase(),
        })
    }
}

/// A single edit operation in a hashline edit batch.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "op")]
pub enum HashlineEditOp {
    /// Replace a single line or a range of lines.
    ///
    /// - `pos`: the anchor line (required)
    /// - `end`: optional end of range (inclusive). If omitted, replaces only `pos`.
    /// - `lines`: the replacement text (newline-separated for multi-line)
    #[serde(rename = "replace")]
    Replace {
        pos: String,
        end: Option<String>,
        lines: String,
    },

    /// Insert lines after the anchor line (or at end of file if pos is omitted).
    ///
    /// - `pos`: anchor line to insert after (optional, defaults to EOF)
    /// - `lines`: the text to insert
    #[serde(rename = "append")]
    Append { pos: Option<String>, lines: String },

    /// Insert lines before the anchor line (or at beginning of file if pos is omitted).
    ///
    /// - `pos`: anchor line to insert before (optional, defaults to BOF)
    /// - `lines`: the text to insert
    #[serde(rename = "prepend")]
    Prepend { pos: Option<String>, lines: String },
}

/// Input for the hashline_edit tool.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HashlineEditInput {
    /// Path to the file to edit
    pub path: String,

    /// Array of edit operations to apply
    pub edits: Vec<HashlineEditOp>,
}

/// Resolved edit with validated HashRefs and computed line ranges.
/// Used internally after validation, before applying edits.
#[derive(Debug, Clone)]
pub struct ResolvedEdit {
    /// The operation type
    pub op: ResolvedOp,
    /// Original index in the edits array (for error reporting)
    pub index: usize,
}

#[derive(Debug, Clone)]
pub enum ResolvedOp {
    Replace {
        start_line: usize,
        end_line: usize,
        new_lines: Vec<String>,
    },
    Append {
        after_line: usize,
        new_lines: Vec<String>,
    },
    Prepend {
        before_line: usize,
        new_lines: Vec<String>,
    },
}

#[cfg(test)]
mod tests {
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
}
