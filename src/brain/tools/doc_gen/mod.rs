//! Document generation tool (#357).
//!
//! `parse_document` reads documents; this is the missing write side. One
//! `generate_document` tool with structured input, dispatching to a native
//! Rust backend per format so document creation works in the distributed
//! binary with zero host dependencies (no Python, no LibreOffice).
//!
//! Backends land format by format; each format module stays small and pure
//! (spec in, file out) so it is testable without the tool plumbing.

pub(crate) mod xlsx;

use super::error::{Result, ToolError};
use super::r#trait::{Tool, ToolCapability, ToolExecutionContext, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

/// Generate Document Tool: creates XLSX (native, with formulas) files from
/// structured content. Further formats get native backends as they land.
pub struct GenerateDocumentTool;

#[derive(Debug, Deserialize)]
pub(crate) struct GenerateDocumentInput {
    /// Output format. Currently: "xlsx".
    pub format: String,

    /// Path the generated file is written to.
    pub output: String,

    /// XLSX: worksheets to create (required when format is "xlsx").
    #[serde(default)]
    pub sheets: Vec<xlsx::SheetSpec>,
}

#[async_trait]
impl Tool for GenerateDocumentTool {
    fn name(&self) -> &str {
        "generate_document"
    }

    fn description(&self) -> &str {
        "Create documents from structured content. Currently supports format \
        \"xlsx\" (Excel workbook, generated natively): pass `sheets`, each with a \
        `name` and `rows` (array of rows, each row an array of cells). A cell is a \
        string, number, or boolean; a string starting with \"=\" is written as a \
        live Excel FORMULA (e.g. \"=SUM(B2:B10)\"), so totals recalculate when the \
        user edits the file. Optional per sheet: `header_bold` (bold first row, \
        default true), `column_widths` (array of numbers). Use this instead of \
        writing CSV when the user asks for a spreadsheet, formulas, or styled \
        tabular output."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "format": {
                    "type": "string",
                    "enum": ["xlsx"],
                    "description": "Output format. \"xlsx\" creates an Excel workbook."
                },
                "output": {
                    "type": "string",
                    "description": "Path to write the generated file to (extension should match the format)."
                },
                "sheets": {
                    "type": "array",
                    "description": "Worksheets to create (xlsx). At least one required.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": {
                                "type": "string",
                                "description": "Worksheet name (max 31 chars, Excel limit)."
                            },
                            "rows": {
                                "type": "array",
                                "description": "Rows of cells. A cell is a string, number, or boolean. Strings starting with \"=\" become live Excel formulas.",
                                "items": {"type": "array"}
                            },
                            "header_bold": {
                                "type": "boolean",
                                "description": "Bold the first row as a header (default: true)."
                            },
                            "column_widths": {
                                "type": "array",
                                "items": {"type": "number"},
                                "description": "Optional column widths, in Excel character units, applied left to right."
                            }
                        },
                        "required": ["name", "rows"]
                    }
                }
            },
            "required": ["format", "output"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::WriteFiles]
    }

    fn requires_approval(&self) -> bool {
        true // Writes a file to disk, same policy as write_file.
    }

    fn validate_input(&self, input: &Value) -> Result<()> {
        let parsed: GenerateDocumentInput = serde_json::from_value(input.clone())
            .map_err(|e| ToolError::InvalidInput(format!("Invalid input: {e}")))?;
        match parsed.format.as_str() {
            "xlsx" => {
                if parsed.sheets.is_empty() {
                    return Err(ToolError::InvalidInput(
                        "format \"xlsx\" requires at least one entry in `sheets`".to_string(),
                    ));
                }
                Ok(())
            }
            other => Err(ToolError::InvalidInput(format!(
                "Unsupported format \"{other}\" (supported: xlsx)"
            ))),
        }
    }

    async fn execute(&self, input: Value, context: &ToolExecutionContext) -> Result<ToolResult> {
        let input: GenerateDocumentInput = serde_json::from_value(input)?;
        let path = super::error::resolve_tool_path(&input.output, &context.working_dir());
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
            && !parent.exists()
        {
            return Ok(ToolResult::error(format!(
                "Output directory does not exist: {}",
                parent.display()
            )));
        }
        match input.format.as_str() {
            "xlsx" => match xlsx::write_workbook(&path, &input.sheets) {
                Ok(summary) => Ok(ToolResult::success(format!(
                    "Created {} ({summary})",
                    path.display()
                ))),
                Err(e) => Ok(ToolResult::error(format!("Failed to create workbook: {e}"))),
            },
            other => Ok(ToolResult::error(format!(
                "Unsupported format \"{other}\" (supported: xlsx)"
            ))),
        }
    }
}
