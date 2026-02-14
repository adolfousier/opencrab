//! Edit File Tool
//!
//! Intelligently modify portions of files (find/replace, line-based edits).

use super::error::{validate_file_path, Result, ToolError};
use super::r#trait::{Tool, ToolCapability, ToolExecutionContext, ToolResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::fs;

/// Edit file tool
pub struct EditTool;

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "operation")]
enum EditOperation {
    /// Replace old_text with new_text
    #[serde(rename = "replace")]
    Replace { old_text: String, new_text: String },

    /// Replace text at specific line range
    #[serde(rename = "replace_lines")]
    ReplaceLines {
        start_line: usize,
        end_line: usize,
        new_text: String,
    },

    /// Insert text at specific line
    #[serde(rename = "insert_line")]
    InsertLine { line: usize, text: String },

    /// Delete lines
    #[serde(rename = "delete_lines")]
    DeleteLines { start_line: usize, end_line: usize },

    /// Regex replace
    #[serde(rename = "regex_replace")]
    RegexReplace {
        pattern: String,
        replacement: String,
    },
}

#[derive(Debug, Deserialize, Serialize)]
struct EditInput {
    /// Path to the file to edit
    path: String,

    /// Edit operation to perform
    #[serde(flatten)]
    operation: EditOperation,

    /// Create backup before editing
    #[serde(default = "default_true")]
    create_backup: bool,
}

fn default_true() -> bool {
    true
}

#[async_trait]
impl Tool for EditTool {
    fn name(&self) -> &str {
        "edit_file"
    }

    fn description(&self) -> &str {
        "Edit a file intelligently using various operations: replace text, replace lines, insert lines, delete lines, or regex replace."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to edit"
                },
                "operation": {
                    "type": "string",
                    "description": "Type of edit operation",
                    "enum": ["replace", "replace_lines", "insert_line", "delete_lines", "regex_replace"]
                },
                "old_text": {
                    "type": "string",
                    "description": "Text to find and replace (for 'replace' operation)"
                },
                "new_text": {
                    "type": "string",
                    "description": "Replacement text (for 'replace' and 'replace_lines' operations)"
                },
                "start_line": {
                    "type": "integer",
                    "description": "Starting line number (0-indexed, for line operations)",
                    "minimum": 0
                },
                "end_line": {
                    "type": "integer",
                    "description": "Ending line number (0-indexed, inclusive, for line operations)",
                    "minimum": 0
                },
                "line": {
                    "type": "integer",
                    "description": "Line number to insert at (0-indexed, for 'insert_line')",
                    "minimum": 0
                },
                "text": {
                    "type": "string",
                    "description": "Text to insert (for 'insert_line')"
                },
                "pattern": {
                    "type": "string",
                    "description": "Regex pattern to match (for 'regex_replace')"
                },
                "replacement": {
                    "type": "string",
                    "description": "Replacement text (for 'regex_replace')"
                },
                "create_backup": {
                    "type": "boolean",
                    "description": "Create backup file before editing (default: true)",
                    "default": true
                }
            },
            "required": ["path", "operation"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![
            ToolCapability::ReadFiles,
            ToolCapability::WriteFiles,
            ToolCapability::SystemModification,
        ]
    }

    fn requires_approval(&self) -> bool {
        true // Editing files requires approval
    }

    fn validate_input(&self, input: &Value) -> Result<()> {
        let _: EditInput = serde_json::from_value(input.clone())
            .map_err(|e| ToolError::InvalidInput(format!("Invalid input: {}", e)))?;
        Ok(())
    }

    async fn execute(&self, input: Value, context: &ToolExecutionContext) -> Result<ToolResult> {
        // Check if in read-only mode (Plan mode)
        if context.read_only_mode {
            return Ok(ToolResult::error(
                "Edit operations are not allowed in Plan mode. \
                 Please approve the plan and switch to execution mode (Ctrl+A) to edit files."
                    .to_string(),
            ));
        }

        let input: EditInput = serde_json::from_value(input)?;

        // Validate path: safety check, existence, and file type
        let path = match validate_file_path(&input.path, &context.working_directory) {
            Ok(p) => p,
            Err(msg) => return Ok(ToolResult::error(msg)),
        };

        // Read file content
        let content = fs::read_to_string(&path).await.map_err(ToolError::Io)?;

        // Create backup if requested
        if input.create_backup {
            let backup_path = path.with_extension(format!(
                "{}.backup",
                path.extension().and_then(|s| s.to_str()).unwrap_or("txt")
            ));
            fs::write(&backup_path, &content)
                .await
                .map_err(ToolError::Io)?;
        }

        // Perform edit operation
        let new_content = match input.operation {
            EditOperation::Replace { old_text, new_text } => {
                if !content.contains(&old_text) {
                    return Ok(ToolResult::error(format!(
                        "Text not found in file: '{}'",
                        old_text
                    )));
                }
                content.replace(&old_text, &new_text)
            }

            EditOperation::ReplaceLines {
                start_line,
                end_line,
                new_text,
            } => {
                let lines: Vec<&str> = content.lines().collect();
                if start_line >= lines.len() || end_line >= lines.len() {
                    return Ok(ToolResult::error(format!(
                        "Line range {}-{} out of bounds (file has {} lines)",
                        start_line,
                        end_line,
                        lines.len()
                    )));
                }
                if start_line > end_line {
                    return Ok(ToolResult::error(
                        "start_line must be <= end_line".to_string(),
                    ));
                }

                let mut new_lines = Vec::new();
                new_lines.extend_from_slice(&lines[..start_line]);
                new_lines.push(&new_text);
                if end_line + 1 < lines.len() {
                    new_lines.extend_from_slice(&lines[end_line + 1..]);
                }
                new_lines.join("\n")
            }

            EditOperation::InsertLine { line, text } => {
                let lines: Vec<&str> = content.lines().collect();
                if line > lines.len() {
                    return Ok(ToolResult::error(format!(
                        "Line {} out of bounds (file has {} lines)",
                        line,
                        lines.len()
                    )));
                }

                let mut new_lines = Vec::new();
                new_lines.extend_from_slice(&lines[..line]);
                new_lines.push(&text);
                new_lines.extend_from_slice(&lines[line..]);
                new_lines.join("\n")
            }

            EditOperation::DeleteLines {
                start_line,
                end_line,
            } => {
                let lines: Vec<&str> = content.lines().collect();
                if start_line >= lines.len() || end_line >= lines.len() {
                    return Ok(ToolResult::error(format!(
                        "Line range {}-{} out of bounds (file has {} lines)",
                        start_line,
                        end_line,
                        lines.len()
                    )));
                }
                if start_line > end_line {
                    return Ok(ToolResult::error(
                        "start_line must be <= end_line".to_string(),
                    ));
                }

                let mut new_lines = Vec::new();
                new_lines.extend_from_slice(&lines[..start_line]);
                if end_line + 1 < lines.len() {
                    new_lines.extend_from_slice(&lines[end_line + 1..]);
                }
                new_lines.join("\n")
            }

            EditOperation::RegexReplace {
                pattern,
                replacement,
            } => {
                let regex = regex::Regex::new(&pattern)
                    .map_err(|e| ToolError::InvalidInput(format!("Invalid regex: {}", e)))?;

                if !regex.is_match(&content) {
                    return Ok(ToolResult::error(format!(
                        "Pattern not found in file: '{}'",
                        pattern
                    )));
                }

                regex
                    .replace_all(&content, replacement.as_str())
                    .to_string()
            }
        };

        // Write modified content
        fs::write(&path, &new_content)
            .await
            .map_err(ToolError::Io)?;

        let lines_before = content.lines().count();
        let lines_after = new_content.lines().count();

        Ok(ToolResult::success(format!(
            "Successfully edited {}. Lines: {} → {}",
            path.display(),
            lines_before,
            lines_after
        )))
    }
}
