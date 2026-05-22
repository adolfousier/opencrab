//! Hashline Edit Tool
//!
//! Hash-anchored file editing: models reference lines by 2-char content hashes
//! instead of reproducing text. Eliminates stale-line errors and reduces token
//! usage, especially for weaker models.

use super::hash::hash_all_lines;
use super::types::{HashRef, HashlineEditInput, HashlineEditOp, ResolvedEdit, ResolvedOp};
use crate::brain::tools::brain_file_safety;
use crate::brain::tools::edit::build_edit_diff;
use crate::brain::tools::error::{Result, ToolError, validate_file_path};
use crate::brain::tools::r#trait::{Tool, ToolCapability, ToolExecutionContext, ToolResult};
use async_trait::async_trait;
use serde_json::Value;
use tokio::fs;

/// Hashline edit tool
pub struct HashlineEditTool;

#[async_trait]
impl Tool for HashlineEditTool {
    fn name(&self) -> &str {
        "hashline_edit"
    }

    fn description(&self) -> &str {
        "Edit a file using hash-anchored line references. Each line is identified by a 2-char \
         content hash (from read_file with hashline=true). Reference lines as LINE#ID (e.g. 12#VK) \
         instead of reproducing text. Stale hashes are rejected before any changes are applied. \
         Supports batch edits (multiple operations in one call)."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to edit"
                },
                "edits": {
                    "type": "array",
                    "description": "Array of edit operations to apply atomically",
                    "items": {
                        "type": "object",
                        "properties": {
                            "op": {
                                "type": "string",
                                "enum": ["replace", "append", "prepend"],
                                "description": "Edit operation type"
                            },
                            "pos": {
                                "type": "string",
                                "description": "Anchor line reference (LINE#ID format, e.g. '12#VK'). Required for replace, optional for append/prepend."
                            },
                            "end": {
                                "type": "string",
                                "description": "End of range for replace (LINE#ID, inclusive). Omit to replace a single line."
                            },
                            "lines": {
                                "type": "string",
                                "description": "Replacement or insertion text. Use \\n for multi-line content."
                            }
                        },
                        "required": ["op", "lines"]
                    },
                    "minItems": 1
                }
            },
            "required": ["path", "edits"]
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
        true
    }

    fn validate_input(&self, input: &Value) -> Result<()> {
        let _: HashlineEditInput = serde_json::from_value(input.clone())
            .map_err(|e| ToolError::InvalidInput(format!("Invalid input: {}", e)))?;
        Ok(())
    }

    async fn execute(&self, input: Value, context: &ToolExecutionContext) -> Result<ToolResult> {
        let input: HashlineEditInput = serde_json::from_value(input)?;

        // Validate path
        let path = match validate_file_path(&input.path, &context.working_dir()) {
            Ok(p) => p,
            Err(msg) => return Ok(ToolResult::error(msg)),
        };

        // Brain-file guardrail
        if brain_file_safety::is_protected_path(&path) {
            return Ok(ToolResult::error(format!(
                "Refusing to edit protected brain file '{}' with hashline_edit. \
                 Use the `write_opencrabs_file` tool instead.",
                path.display()
            )));
        }

        // Read file
        let content = fs::read_to_string(&path).await.map_err(ToolError::Io)?;
        let original_lines: Vec<&str> = content.lines().collect();
        let total_lines = original_lines.len();

        // Compute hashes for all lines
        let line_hashes = hash_all_lines(&content);

        // Build a reverse lookup: hash → list of line numbers (for collision detection and lookup)
        let mut hash_to_lines: std::collections::HashMap<&str, Vec<usize>> =
            std::collections::HashMap::new();
        for (num, hash) in &line_hashes {
            hash_to_lines.entry(hash.as_str()).or_default().push(*num);
        }

        // Phase 1: Validate all hash references and resolve to line numbers
        let mut resolved = Vec::with_capacity(input.edits.len());
        for (i, edit) in input.edits.iter().enumerate() {
            match resolve_edit(edit, i, &hash_to_lines, total_lines)? {
                Ok(resolved_edit) => resolved.push(resolved_edit),
                Err(error_msg) => return Ok(ToolResult::error(error_msg)),
            }
        }

        // Phase 2: Sort bottom-up (highest line number first) to preserve line numbers
        resolved.sort_by(|a, b| {
            let line_a = edit_sort_line(a);
            let line_b = edit_sort_line(b);
            line_b.cmp(&line_a) // descending
        });

        // Phase 3: Detect overlapping ranges
        if let Some(overlap_err) = detect_overlaps(&resolved) {
            return Ok(ToolResult::error(overlap_err));
        }

        // Phase 4: Apply edits (bottom-up, so line numbers stay stable)
        let mut result_lines: Vec<String> = original_lines.iter().map(|s| s.to_string()).collect();

        for edit in &resolved {
            apply_edit(&mut result_lines, edit);
        }

        let new_content = result_lines.join("\n");

        // Preserve trailing newline if original had one
        let new_content = if content.ends_with('\n') && !new_content.ends_with('\n') {
            format!("{}\n", new_content)
        } else {
            new_content
        };

        // Write
        fs::write(&path, &new_content)
            .await
            .map_err(ToolError::Io)?;

        let lines_before = original_lines.len();
        let lines_after = new_content.lines().count();
        let diff = build_edit_diff(&content, &new_content);

        let mut output = format!(
            "Successfully edited {} (hashline). Lines: {} → {}\n",
            path.display(),
            lines_before,
            lines_after
        );
        output.push_str(&diff);

        Ok(ToolResult::success(output))
    }
}

/// Resolve a single edit operation, validating all hash references.
///
/// Returns Ok(Ok(resolved)) on success, Ok(Err(message)) on validation failure
/// (so we can return a user-friendly error without propagating ToolError).
fn resolve_edit(
    edit: &HashlineEditOp,
    index: usize,
    hash_to_lines: &std::collections::HashMap<&str, Vec<usize>>,
    total_lines: usize,
) -> Result<std::result::Result<ResolvedEdit, String>> {
    match edit {
        HashlineEditOp::Replace { pos, end, lines } => {
            let pos_ref = match HashRef::parse(pos) {
                Ok(r) => r,
                Err(e) => return Ok(Err(format!("Edit #{}: {}", index + 1, e))),
            };

            // Validate pos hash and get line number
            let start_line = match validate_hash(&pos_ref, hash_to_lines, total_lines) {
                Ok(line) => line,
                Err(e) => return Ok(Err(format!("Edit #{}: {}", index + 1, e))),
            };

            let end_line = if let Some(end_str) = end {
                let end_ref = match HashRef::parse(end_str) {
                    Ok(r) => r,
                    Err(e) => return Ok(Err(format!("Edit #{}: {}", index + 1, e))),
                };
                let end_line = match validate_hash(&end_ref, hash_to_lines, total_lines) {
                    Ok(line) => line,
                    Err(e) => return Ok(Err(format!("Edit #{}: {}", index + 1, e))),
                };
                if end_line < start_line {
                    return Ok(Err(format!(
                        "Edit #{}: end line ({}) must be >= start line ({})",
                        index + 1,
                        end_line,
                        start_line
                    )));
                }
                end_line
            } else {
                start_line
            };

            let new_lines = strip_hashline_prefixes(lines);

            Ok(Ok(ResolvedEdit {
                op: ResolvedOp::Replace {
                    start_line,
                    end_line,
                    new_lines,
                },
                index,
            }))
        }

        HashlineEditOp::Append { pos, lines } => {
            let after_line = if let Some(pos_str) = pos {
                let pos_ref = match HashRef::parse(pos_str) {
                    Ok(r) => r,
                    Err(e) => return Ok(Err(format!("Edit #{}: {}", index + 1, e))),
                };
                match validate_hash(&pos_ref, hash_to_lines, total_lines) {
                    Ok(line) => line,
                    Err(e) => return Ok(Err(format!("Edit #{}: {}", index + 1, e))),
                }
            } else {
                total_lines // append at EOF
            };

            let new_lines = strip_hashline_prefixes(lines);

            Ok(Ok(ResolvedEdit {
                op: ResolvedOp::Append {
                    after_line,
                    new_lines,
                },
                index,
            }))
        }

        HashlineEditOp::Prepend { pos, lines } => {
            let before_line = if let Some(pos_str) = pos {
                let pos_ref = match HashRef::parse(pos_str) {
                    Ok(r) => r,
                    Err(e) => return Ok(Err(format!("Edit #{}: {}", index + 1, e))),
                };
                match validate_hash(&pos_ref, hash_to_lines, total_lines) {
                    Ok(line) => line,
                    Err(e) => return Ok(Err(format!("Edit #{}: {}", index + 1, e))),
                }
            } else {
                1 // prepend at BOF
            };

            let new_lines = strip_hashline_prefixes(lines);

            Ok(Ok(ResolvedEdit {
                op: ResolvedOp::Prepend {
                    before_line,
                    new_lines,
                },
                index,
            }))
        }
    }
}

/// Validate that a HashRef's hash exists in the file and is unique.
///
/// Returns the line number where the hash was found.
///
/// Checks for:
/// 1. Hash not found in file
/// 2. Hash collision (same hash on multiple lines) → escalate to edit_file
fn validate_hash(
    href: &HashRef,
    hash_to_lines: &std::collections::HashMap<&str, Vec<usize>>,
    _total_lines: usize,
) -> std::result::Result<usize, String> {
    // Look up which line(s) have this hash
    match hash_to_lines.get(href.hash.as_str()) {
        None => Err(format!(
            "Hash #{} not found in file. The file may have changed since your last read. \
             Re-read with hashline=true to get updated references.",
            href.hash
        )),
        Some(lines_with_hash) if lines_with_hash.len() > 1 => {
            let line_list: Vec<String> = lines_with_hash.iter().map(|l| l.to_string()).collect();
            Err(format!(
                "Hash collision: #{} appears on {} lines ({}). \
                 This hash is ambiguous and cannot safely identify a single line. \
                 Use the `edit_file` tool with search/replace instead of hashline_edit.",
                href.hash,
                lines_with_hash.len(),
                line_list.join(", ")
            ))
        }
        Some(lines_with_hash) => {
            // Unique hash, return the line number
            Ok(lines_with_hash[0])
        }
    }
}

/// Get the sort key for an edit (used for bottom-up ordering).
fn edit_sort_line(edit: &ResolvedEdit) -> usize {
    match &edit.op {
        ResolvedOp::Replace { start_line, .. } => *start_line,
        ResolvedOp::Append { after_line, .. } => *after_line,
        ResolvedOp::Prepend { before_line, .. } => *before_line,
    }
}

/// Detect overlapping ranges in a set of resolved edits.
/// Returns Some(error_message) if overlaps found, None if clean.
fn detect_overlaps(edits: &[ResolvedEdit]) -> Option<String> {
    // Build a list of (start, end) ranges for each edit
    let mut ranges: Vec<(usize, usize, usize)> = edits
        .iter()
        .map(|e| {
            let (start, end) = match &e.op {
                ResolvedOp::Replace {
                    start_line,
                    end_line,
                    ..
                } => (*start_line, *end_line),
                ResolvedOp::Append { after_line, .. } => (*after_line + 1, *after_line + 1),
                ResolvedOp::Prepend { before_line, .. } => (*before_line, *before_line),
            };
            (start, end, e.index + 1)
        })
        .collect();

    // Sort by start line ascending
    ranges.sort_by_key(|r| r.0);

    // Check adjacent ranges for overlap
    for i in 0..ranges.len().saturating_sub(1) {
        let (_, end_a, idx_a) = ranges[i];
        let (start_b, _, idx_b) = ranges[i + 1];
        if end_a >= start_b {
            return Some(format!(
                "Overlapping edits: edit #{} (ending at line {}) overlaps with edit #{} (starting at line {}). \
                 Adjust the ranges so they don't overlap.",
                idx_a, end_a, idx_b, start_b
            ));
        }
    }

    None
}

/// Apply a single resolved edit to the line buffer.
/// Edits are applied bottom-up (highest line first) so earlier line numbers stay stable.
fn apply_edit(lines: &mut Vec<String>, edit: &ResolvedEdit) {
    match &edit.op {
        ResolvedOp::Replace {
            start_line,
            end_line,
            new_lines,
        } => {
            let start_idx = start_line - 1; // convert to 0-indexed
            let end_idx = *end_line; // exclusive end for drain

            // Clamp to valid range
            let start_idx = start_idx.min(lines.len());
            let end_idx = end_idx.min(lines.len());

            // Remove old lines
            lines.drain(start_idx..end_idx);

            // Insert new lines
            for (i, new_line) in new_lines.iter().enumerate() {
                lines.insert(start_idx + i, new_line.clone());
            }
        }

        ResolvedOp::Append {
            after_line,
            new_lines,
        } => {
            let insert_idx = (*after_line).min(lines.len());
            for (i, new_line) in new_lines.iter().enumerate() {
                lines.insert(insert_idx + i, new_line.clone());
            }
        }

        ResolvedOp::Prepend {
            before_line,
            new_lines,
        } => {
            let insert_idx = (before_line - 1).min(lines.len());
            for (i, new_line) in new_lines.iter().enumerate() {
                lines.insert(insert_idx + i, new_line.clone());
            }
        }
    }
}

/// Strip hashline prefixes from content lines if the model accidentally included them.
///
/// Detects lines starting with `DIGITS#XX|` pattern and strips the prefix.
fn strip_hashline_prefixes(text: &str) -> Vec<String> {
    text.lines()
        .map(|line| {
            // Check for pattern: digits + '#' + 2 chars + '|'
            if let Some(hash_pos) = line.find('#') {
                let before = &line[..hash_pos];
                let after = &line[hash_pos + 1..];

                // before must be all digits
                if !before.is_empty()
                    && before.chars().all(|c| c.is_ascii_digit())
                    && after.len() >= 3
                    && after.as_bytes()[0].is_ascii_uppercase()
                    && after.as_bytes()[1].is_ascii_uppercase()
                    && after.as_bytes()[2] == b'|'
                {
                    return after[3..].to_string();
                }
            }
            line.to_string()
        })
        .collect()
}
