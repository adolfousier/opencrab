//! Document generation tool (#357).
//!
//! `parse_document` reads documents; this is the missing write side. One
//! `generate_document` tool with structured input, dispatching to a native
//! Rust backend per format so document creation works in the distributed
//! binary with zero host dependencies (no Python, no LibreOffice).
//!
//! Backends land format by format; each format module stays small and pure
//! (spec in, file out) so it is testable without the tool plumbing.

pub(crate) mod docx;
pub(crate) mod xlsx;

use super::error::{Result, ToolError};
use super::r#trait::{Tool, ToolCapability, ToolExecutionContext, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

/// Generate Document Tool: creates XLSX (native, with formulas) and DOCX
/// (native, styled blocks) files from structured content. Further formats
/// get native backends as they land.
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

    /// DOCX: content blocks in order (required when format is "docx").
    #[serde(default)]
    pub blocks: Vec<docx::BlockSpec>,
}

#[async_trait]
impl Tool for GenerateDocumentTool {
    fn name(&self) -> &str {
        "generate_document"
    }

    fn description(&self) -> &str {
        "Create documents from structured content. Formats: \"xlsx\" and \"docx\", \
        both generated natively. \
        XLSX: pass `sheets`, each with a `name` and `rows` (array of rows, each row \
        an array of cells). A cell is a string, number, or boolean; a string \
        starting with \"=\" is written as a live Excel FORMULA (e.g. \
        \"=SUM(B2:B10)\"), so totals recalculate when the user edits the file. \
        Optional per sheet: `header_bold` (default true), `column_widths`. \
        DOCX: pass `blocks` in order, each one of: {type:\"heading\", text, level \
        1-3}, {type:\"paragraph\", text, bold?}, {type:\"list\", items:[...], \
        ordered?}, {type:\"table\", rows:[[...]], header_bold?}. Headings become \
        real Word styles, lists real numbering. \
        Use this instead of CSV/markdown files whenever the user asks for a \
        spreadsheet, formulas, or a Word document."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "format": {
                    "type": "string",
                    "enum": ["xlsx", "docx"],
                    "description": "Output format. \"xlsx\" creates an Excel workbook, \"docx\" a Word document."
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
                },
                "blocks": {
                    "type": "array",
                    "description": "Content blocks in order (docx). At least one required.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "type": {
                                "type": "string",
                                "enum": ["heading", "paragraph", "list", "table"],
                                "description": "Block kind."
                            },
                            "text": {"type": "string", "description": "Text for heading/paragraph blocks."},
                            "level": {"type": "integer", "minimum": 1, "maximum": 3, "description": "Heading level (default 1)."},
                            "bold": {"type": "boolean", "description": "Bold the paragraph text."},
                            "items": {"type": "array", "items": {"type": "string"}, "description": "List items."},
                            "ordered": {"type": "boolean", "description": "Numbered list instead of bullets (default false)."},
                            "rows": {"type": "array", "items": {"type": "array"}, "description": "Table rows of cells."},
                            "header_bold": {"type": "boolean", "description": "Bold the first table row (default true)."}
                        },
                        "required": ["type"]
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
            "docx" => {
                if parsed.blocks.is_empty() {
                    return Err(ToolError::InvalidInput(
                        "format \"docx\" requires at least one entry in `blocks`".to_string(),
                    ));
                }
                Ok(())
            }
            other => Err(ToolError::InvalidInput(format!(
                "Unsupported format \"{other}\" (supported: xlsx, docx)"
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
            "docx" => match docx::write_document(&path, &input.blocks) {
                Ok(summary) => Ok(ToolResult::success(format!(
                    "Created {} ({summary})",
                    path.display()
                ))),
                Err(e) => Ok(ToolResult::error(format!("Failed to create document: {e}"))),
            },
            other => Ok(ToolResult::error(format!(
                "Unsupported format \"{other}\" (supported: xlsx, docx)"
            ))),
        }
    }
}
