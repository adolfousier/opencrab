//! Write OpenCrabs File Tool
//!
//! Writes or edits any file within `~/.opencrabs/` (brain files like
//! MEMORY.md, USER.md, config files like commands.toml, memory logs, and
//! any other app-owned files). The standard `edit_file`/`write_file`
//! tools refuse to touch protected brain files (issue #91 guardrail)
//! and route the caller here instead. This tool enforces append-only
//! writes, dedup-aware shrinking, and saves a `.bak` snapshot before
//! every change.

use super::error::Result;
use super::r#trait::{Tool, ToolCapability, ToolExecutionContext, ToolResult};
use async_trait::async_trait;
use serde_json::Value;

pub struct WriteOpenCrabsFileTool;

/// Validate that `path` is a safe relative path within `~/.opencrabs/`.
/// Prevents path traversal outside the app home directory.
pub(super) fn validate_opencrabs_path(path: &str) -> std::result::Result<(), String> {
    if path.is_empty() {
        return Err("path is required".into());
    }
    // Reject absolute paths — must be relative to ~/.opencrabs/
    if path.starts_with('/') || path.starts_with('~') {
        return Err(format!(
            "Use a relative path (e.g. \"MEMORY.md\" or \"memory/2026-03-02.md\"), \
             not an absolute path '{}'",
            path
        ));
    }
    // Reject traversal attempts
    if path.contains("..") {
        return Err(format!(
            "'{}' contains '..' — path traversal is not allowed",
            path
        ));
    }
    // Reject null bytes
    if path.contains('\0') {
        return Err("path contains null bytes".into());
    }
    Ok(())
}

#[async_trait]
impl Tool for WriteOpenCrabsFileTool {
    fn name(&self) -> &str {
        "write_opencrabs_file"
    }

    fn description(&self) -> &str {
        "Write or edit any file within the OpenCrabs home directory (~/.opencrabs/). \
         Use this for brain files (MEMORY.md, USER.md, AGENTS.md, SOUL.md, etc.), \
         config files (commands.toml), memory logs, and any other app files. \
         The standard edit_file/write_file tools cannot reach ~/.opencrabs/ — use this instead. \
         Provide a relative path (e.g. \"MEMORY.md\" or \"memory/note.md\"). \
         Supports three operations: \
         \"overwrite\" replaces entire file content, \
         \"append\" adds text to the end, \
         \"replace\" does a find-and-replace within the file. \
         \
         Protected brain files are append-only by default. To shrink/clean up a brain file \
         (remove outdated content), set cleanup_intent=true — this requires explicit user \
         approval and is NOT available in autonomous RSI operations."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Relative path within ~/.opencrabs/ (e.g. \"MEMORY.md\", \"memory/2026-03-02.md\", \"commands.toml\"). No leading slash, no '..'."
                },
                "operation": {
                    "type": "string",
                    "enum": ["overwrite", "append", "replace"],
                    "description": "\"overwrite\": replace entire file. \"append\": add to end. \"replace\": find old_text and replace with new_text."
                },
                "content": {
                    "type": "string",
                    "description": "Content to write (required for overwrite and append)."
                },
                "old_text": {
                    "type": "string",
                    "description": "Text to find (required for replace)."
                },
                "new_text": {
                    "type": "string",
                    "description": "Replacement text (required for replace)."
                },
                "dedup_intent": {
                    "type": "boolean",
                    "description": "Set to true ONLY when shrinking a protected brain file (TOOLS.md, MEMORY.md, SOUL.md, USER.md, AGENTS.md, CODE.md, SECURITY.md, BOOT.md, IDENTITY.md) to deduplicate. Brain files are append-only — any overwrite/replace whose result is shorter than the existing file is rejected unless dedup_intent=true AND every original line still appears in the result."
                },
                "cleanup_intent": {
                    "type": "boolean",
                    "description": "Set to true ONLY when you need to intentionally clean up a protected brain file (remove outdated content, consolidate sections, etc.). This bypasses the append-only restriction and allows shrinking. Requires explicit user approval (this tool has requires_approval: true). This parameter is NOT available in the autonomous RSI self_improve tool — only user-initiated operations can clean up brain files."
                }
            },
            "required": ["path", "operation"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::WriteFiles]
    }

    fn requires_approval(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value, _ctx: &ToolExecutionContext) -> Result<ToolResult> {
        let path_str = input
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();

        if let Err(e) = validate_opencrabs_path(path_str) {
            return Ok(ToolResult::error(e));
        }

        let operation = input
            .get("operation")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();

        let home = crate::config::opencrabs_home();
        let full_path = home.join(path_str);

        match operation {
            "overwrite" => {
                let content = match input.get("content").and_then(|v| v.as_str()) {
                    Some(c) => c,
                    None => {
                        return Ok(ToolResult::error(
                            "content is required for overwrite".into(),
                        ));
                    }
                };
                // Append-only contract: overwriting a protected brain file
                // is only allowed when it doesn't lose bytes. The 2026-04-26
                // RSI rewrite of TOOLS.md happened by passing the whole file
                // as `old_content` to a `replace` — but `overwrite` is the
                // even more direct path to the same damage.
                use crate::brain::tools::brain_file_safety;
                if brain_file_safety::is_protected_path(&full_path) {
                    let existing = std::fs::read_to_string(&full_path).unwrap_or_default();
                    let dedup_intent = input
                        .get("dedup_intent")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let cleanup_intent = input
                        .get("cleanup_intent")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    if let brain_file_safety::ShrinkCheck::Rejected { message } =
                        brain_file_safety::check_no_shrink(
                            &full_path,
                            &existing,
                            content,
                            dedup_intent,
                            cleanup_intent,
                        )
                    {
                        return Ok(ToolResult::error(message));
                    }
                }
                if let Some(parent) = full_path.parent()
                    && let Err(e) = std::fs::create_dir_all(parent)
                {
                    return Ok(ToolResult::error(format!(
                        "Failed to create directory: {}",
                        e
                    )));
                }
                if let Err(e) = brain_file_safety::backup_before_write(&full_path) {
                    tracing::warn!("write_opencrabs_file: backup failed for {path_str}: {e}");
                }
                match std::fs::write(&full_path, content) {
                    Ok(()) => Ok(ToolResult::success(format!(
                        "Wrote {} bytes to ~/.opencrabs/{}",
                        content.len(),
                        path_str
                    ))),
                    Err(e) => Ok(ToolResult::error(format!(
                        "Failed to write {}: {}",
                        path_str, e
                    ))),
                }
            }

            "append" => {
                let content = match input.get("content").and_then(|v| v.as_str()) {
                    Some(c) => c,
                    None => return Ok(ToolResult::error("content is required for append".into())),
                };
                use crate::brain::tools::brain_file_safety::{
                    self, AppendDedup, filter_duplicate_append,
                };
                // Dedup check for protected brain files: extract only genuinely
                // new paragraphs instead of blindly appending everything.
                let effective_content = if brain_file_safety::is_protected_path(&full_path) {
                    let existing = std::fs::read_to_string(&full_path).unwrap_or_default();
                    match filter_duplicate_append(&existing, content) {
                        AppendDedup::AllNew => content.to_string(),
                        AppendDedup::Filtered {
                            filtered_content,
                            skipped_paragraphs,
                        } => {
                            tracing::info!(
                                "write_opencrabs_file: filtered {skipped_paragraphs} duplicate paragraph(s) from append to {path_str}"
                            );
                            filtered_content
                        }
                        AppendDedup::AllDuplicate => {
                            return Ok(ToolResult::error(format!(
                                "Content already exists in {}. Skipping duplicate append. \
                                 Use replace if you want to update existing content.",
                                path_str
                            )));
                        }
                    }
                } else {
                    content.to_string()
                };
                if let Some(parent) = full_path.parent()
                    && let Err(e) = std::fs::create_dir_all(parent)
                {
                    return Ok(ToolResult::error(format!(
                        "Failed to create directory: {}",
                        e
                    )));
                }
                if let Err(e) = brain_file_safety::backup_before_write(&full_path) {
                    tracing::warn!("write_opencrabs_file: backup failed for {path_str}: {e}");
                }
                use std::io::Write;
                match std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&full_path)
                {
                    Ok(mut f) => match f.write_all(effective_content.as_bytes()) {
                        Ok(()) => Ok(ToolResult::success(format!(
                            "Appended {} bytes to ~/.opencrabs/{}",
                            effective_content.len(),
                            path_str
                        ))),
                        Err(e) => Ok(ToolResult::error(format!(
                            "Failed to append to {}: {}",
                            path_str, e
                        ))),
                    },
                    Err(e) => Ok(ToolResult::error(format!(
                        "Failed to open {}: {}",
                        path_str, e
                    ))),
                }
            }

            "replace" => {
                let old_text = match input.get("old_text").and_then(|v| v.as_str()) {
                    Some(t) => t,
                    None => {
                        return Ok(ToolResult::error("old_text is required for replace".into()));
                    }
                };
                let new_text = match input.get("new_text").and_then(|v| v.as_str()) {
                    Some(t) => t,
                    None => {
                        return Ok(ToolResult::error("new_text is required for replace".into()));
                    }
                };
                let existing = match std::fs::read_to_string(&full_path) {
                    Ok(s) => s,
                    Err(_) => {
                        return Ok(ToolResult::error(format!(
                            "~/.opencrabs/{} not found. Use overwrite to create it.",
                            path_str
                        )));
                    }
                };
                if !existing.contains(old_text) {
                    return Ok(ToolResult::error(format!(
                        "old_text not found in {}. No changes made.",
                        path_str
                    )));
                }
                let updated = existing.replacen(old_text, new_text, 1);
                use crate::brain::tools::brain_file_safety;
                let dedup_intent = input
                    .get("dedup_intent")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let cleanup_intent = input
                    .get("cleanup_intent")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if let brain_file_safety::ShrinkCheck::Rejected { message } =
                    brain_file_safety::check_no_shrink(
                        &full_path,
                        &existing,
                        &updated,
                        dedup_intent,
                        cleanup_intent,
                    )
                {
                    return Ok(ToolResult::error(message));
                }
                if let Err(e) = brain_file_safety::backup_before_write(&full_path) {
                    tracing::warn!("write_opencrabs_file: backup failed for {path_str}: {e}");
                }
                match std::fs::write(&full_path, &updated) {
                    Ok(()) => Ok(ToolResult::success(format!(
                        "Replaced text in ~/.opencrabs/{}",
                        path_str
                    ))),
                    Err(e) => Ok(ToolResult::error(format!(
                        "Failed to write {}: {}",
                        path_str, e
                    ))),
                }
            }

            other => Ok(ToolResult::error(format!(
                "Unknown operation '{}'. Use: overwrite, append, replace.",
                other
            ))),
        }
    }
}

#[cfg(test)]
#[path = "write_opencrabs_file_tests.rs"]
mod write_opencrabs_file_tests;
