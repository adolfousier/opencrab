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
        serde_json::json!({
            "type": "object",
            "properties": {
                "format": {
                    "type": "string",
                    "enum": ["xlsx", "docx", "pdf", "pptx"],
                    "description": "Output format. \"xlsx\" creates an Excel workbook, \"docx\" a Word document, \"pdf\" a PDF, \"pptx\" a PowerPoint deck (requires python-pptx on the host)."
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
                "title": {
                    "type": "string",
                    "description": "PDF metadata title (pdf only; defaults to the output file stem)."
                },
                "blocks": {
                    "type": "array",
                    "description": "Content blocks in order (docx/pdf). At least one required.",
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
                            "path": {"type": "string", "description": "Local PNG/JPEG path for image blocks."},
                            "width_mm": {"type": "number", "description": "Rendered image width in millimeters (aspect preserved; clamped to the page body). Default: natural size, downscaled to fit."},
                            "caption": {"type": "string", "description": "Caption text under an image block."},
                            "header_bold": {"type": "boolean", "description": "Bold the first table row (default true)."}
                        },
                        "required": ["type"]
                    }
                },
                "style": {
                    "type": "object",
                    "description": "Visual styling, interpreted per format. All fields optional. PDF: accent_color, text_color, page_header{text,logo_path}, page_footer{text,page_numbers}, zebra_rows, orientation (portrait/landscape, default portrait), page_size{width_mm,height_mm} (custom dimensions; takes precedence over orientation). XLSX: header_fill, header_font_color, zebra_rows, freeze_header, autofilter, tab_color (hex colors); per-sheet column_formats on each sheet. DOCX: accent_color (heading color), page_header, page_footer, table_header_fill, zebra_rows, orientation (portrait/landscape), page_size{width_mm,height_mm}. PPTX: template_path (existing .pptx whose master/branding the slides inherit; best branding lever), accent_color (title color), slide_size (preset \"16:9\"/\"4:3\"/\"square\" or {width_inches,height_inches}; ignored with template_path), per-slide layout index. Use when the user wants polished or branded output.",
                    "properties": {
                        "accent_color": {"type": "string", "description": "Hex color (\"#0A84FF\") for headings, H1 underline bar, and table header separator."},
                        "text_color": {"type": "string", "description": "Hex color for body text (default near-black)."},
                        "page_header": {
                            "type": "object",
                            "properties": {
                                "text": {"type": "string", "description": "Brand/report name shown at the top of every page with an accent rule."},
                                "logo_path": {"type": "string", "description": "Local PNG/JPEG path drawn in the page header band."}
                            }
                        },
                        "page_footer": {
                            "type": "object",
                            "properties": {
                                "text": {"type": "string", "description": "Footer text on every page."},
                                "page_numbers": {"type": "boolean", "description": "Render \"Page N of M\" bottom-right."}
                            }
                        },
                        "zebra_rows": {"type": "boolean", "description": "Alternating light fills behind table rows."},
                        "orientation": {
                            "type": "string",
                            "enum": ["portrait", "landscape"],
                            "description": "Page orientation for PDF output (default portrait). Ignored when page_size is set."
                        },
                        "page_size": {
                            "type": "object",
                            "description": "Custom page dimensions in mm for PDF output (takes precedence over orientation).",
                            "properties": {
                                "width_mm": {"type": "number", "minimum": 100, "description": "Page width in millimeters."},
                                "height_mm": {"type": "number", "minimum": 100, "description": "Page height in millimeters."}
                            },
                            "required": ["width_mm", "height_mm"]
                        }
                    }
                },
                "slides": {
                    "type": "array",
                    "description": "Slides to create (pptx). At least one required.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "title": {"type": "string", "description": "Slide title."},
                            "bullets": {"type": "array", "items": {"type": "string"}, "description": "Bullet lines for the slide body."},
                            "notes": {"type": "string", "description": "Optional speaker notes."}
                        },
                        "required": ["title"]
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
