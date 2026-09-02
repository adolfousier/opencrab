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
mod input;
pub(crate) mod pdf;
pub(crate) mod pptx;
mod schema;
pub(crate) mod xlsx;

use super::error::{Result, ToolError};
use super::r#trait::{Tool, ToolCapability, ToolExecutionContext, ToolResult};
use async_trait::async_trait;
use input::GenerateDocumentInput;
use serde_json::Value;

/// Generate Document Tool: creates XLSX (native, with formulas), DOCX and
/// PDF (native, styled blocks) files from structured content, plus PPTX via
/// the host's python-pptx when available. The native backends run inside
/// the binary with zero host dependencies.
pub struct GenerateDocumentTool;

#[async_trait]
impl Tool for GenerateDocumentTool {
    fn name(&self) -> &str {
        "generate_document"
    }

    fn description(&self) -> &str {
        "Create documents from structured content. Formats: \"xlsx\", \"docx\", \
        \"pdf\" (all generated natively) and \"pptx\" (needs python-pptx on the \
        host). \
        XLSX: pass `sheets`, each with a `name` and `rows` (array of rows, each row \
        an array of cells). A cell is a string, number, or boolean; a string \
        starting with \"=\" is written as a live Excel FORMULA (e.g. \
        \"=SUM(B2:B10)\"), so totals recalculate when the user edits the file. \
        Optional per sheet: `header_bold` (default true), `column_widths`. \
        DOCX and PDF: pass `blocks` in order, each one of: {type:\"heading\", text, \
        level 1-3}, {type:\"paragraph\", text, bold?}, {type:\"list\", items:[...], \
        ordered?}, {type:\"table\", rows:[[...]], header_bold?}, {type:\"image\", \
        path (local PNG/JPEG), width_mm?, caption?} for embedded visuals \
        (charts, diagrams, logos) inline in the document flow. In Word output, \
        headings become real styles and lists real numbering; PDF output is A4 \
        with automatic wrapping and page breaks (optional `title` sets the PDF \
        metadata title; optional `style` adds brand colors, a page header/footer \
        with page numbers, and zebra table rows: use it when the user wants a \
        polished or branded report). \
        PPTX: pass `slides`, each {title, bullets?:[...], notes?}. \
        Use this instead of CSV/markdown files whenever the user asks for a \
        spreadsheet, formulas, a Word document, a PDF, or a slide deck. \
        On channels, deliver the generated file as a downloadable attachment in \
        the same turn: telegram_send `send_document` (document_url takes the \
        local path), whatsapp_send `send_document`, or discord_send `send_file`. \
        ORDERING: send the file(s) BEFORE composing your final text answer, and \
        always end the turn with a short closing text after the attachments — \
        never leave files dangling as the last thing in the chat. \
        Do not just paste the file path unless the user asked where it is saved."
    }

    fn input_schema(&self) -> Value {
        schema::input_schema()
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
            "docx" | "pdf" => {
                if parsed.blocks.is_empty() {
                    return Err(ToolError::InvalidInput(format!(
                        "format \"{}\" requires at least one entry in `blocks`",
                        parsed.format
                    )));
                }
                Ok(())
            }
            "pptx" => {
                if parsed.slides.is_empty() {
                    return Err(ToolError::InvalidInput(
                        "format \"pptx\" requires at least one entry in `slides`".to_string(),
                    ));
                }
                Ok(())
            }
            other => Err(ToolError::InvalidInput(format!(
                "Unsupported format \"{other}\" (supported: xlsx, docx, pdf, pptx)"
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
            "xlsx" => {
                let style: xlsx::XlsxStyle = match input.style.clone() {
                    Some(v) => match serde_json::from_value(v) {
                        Ok(st) => st,
                        Err(e) => {
                            return Ok(ToolResult::error(format!("Invalid xlsx style: {e}")));
                        }
                    },
                    None => xlsx::XlsxStyle::default(),
                };
                match xlsx::write_workbook(&path, &input.sheets, &style) {
                    Ok(summary) => Ok(ToolResult::success(format!(
                        "Created {} ({summary})",
                        path.display()
                    ))),
                    Err(e) => Ok(ToolResult::error(format!("Failed to create workbook: {e}"))),
                }
            }
            "docx" => {
                let style: docx::DocxStyle = match input.style.clone() {
                    Some(v) => match serde_json::from_value(v) {
                        Ok(st) => st,
                        Err(e) => {
                            return Ok(ToolResult::error(format!("Invalid docx style: {e}")));
                        }
                    },
                    None => docx::DocxStyle::default(),
                };
                match docx::write_document(&path, &input.blocks, &style) {
                    Ok(summary) => Ok(ToolResult::success(format!(
                        "Created {} ({summary})",
                        path.display()
                    ))),
                    Err(e) => Ok(ToolResult::error(format!("Failed to create document: {e}"))),
                }
            }
            "pdf" => {
                let title = input.title.clone().unwrap_or_else(|| {
                    path.file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "Document".to_string())
                });
                let style: pdf::StyleSpec = match input.style.clone() {
                    Some(v) => match serde_json::from_value(v) {
                        Ok(st) => st,
                        Err(e) => {
                            return Ok(ToolResult::error(format!("Invalid pdf style: {e}")));
                        }
                    },
                    None => pdf::StyleSpec::default(),
                };
                match pdf::write_pdf(&path, &input.blocks, &title, &style) {
                    Ok(summary) => Ok(ToolResult::success(format!(
                        "Created {} ({summary})",
                        path.display()
                    ))),
                    Err(e) => Ok(ToolResult::error(format!("Failed to create PDF: {e}"))),
                }
            }
            "pptx" => {
                let style: pptx::PptxStyle = match input.style.clone() {
                    Some(v) => match serde_json::from_value(v) {
                        Ok(st) => st,
                        Err(e) => {
                            return Ok(ToolResult::error(format!("Invalid pptx style: {e}")));
                        }
                    },
                    None => pptx::PptxStyle::default(),
                };
                match pptx::write_deck(&path, &input.slides, &style).await {
                    Ok(summary) => Ok(ToolResult::success(format!(
                        "Created {} ({summary})",
                        path.display()
                    ))),
                    Err(e) => Ok(ToolResult::error(format!("Failed to create deck: {e}"))),
                }
            }
            other => Ok(ToolResult::error(format!(
                "Unsupported format \"{other}\" (supported: xlsx, docx, pdf, pptx)"
            ))),
        }
    }
}
