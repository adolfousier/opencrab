//! PDF-to-Images Tool
//!
//! Renders PDF pages to PNG files so the agent can *see* figures,
//! screenshots, diagrams, charts, or scanned content that plain text
//! extraction (`parse_document`) silently drops. Returns the rendered
//! page-image paths; the agent then views them ONE AT A TIME with
//! `analyze_image`, exactly like the auto-ingest path for scanned PDFs.
//!
//! Why a separate tool: a PDF with plenty of text but embedded images
//! takes the text-extraction path on ingest, so the images never reach
//! a vision model. This tool is the on-demand escape hatch for that case.

use super::error::{Result, ToolError};
use super::r#trait::{Tool, ToolCapability, ToolExecutionContext, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use std::path::Path;

/// Upper bound on pages rendered in a single call, to cap memory and
/// disk. Mirrors the auto-ingest cap in `utils::file_extract`.
const MAX_PAGES: usize = 100;

pub struct PdfToImagesTool;

#[derive(Debug, Deserialize)]
struct PdfToImagesInput {
    /// Path to the PDF file.
    path: String,

    /// Optional: specific pages to render (1-indexed).
    #[serde(default)]
    pages: Option<Vec<usize>>,

    /// Optional: page range string like "1-5", "3,7,10-12".
    /// Merged with `pages`. Omit to render from page 1 up to the cap.
    #[serde(default)]
    page_range: Option<String>,
}

#[async_trait]
impl Tool for PdfToImagesTool {
    fn name(&self) -> &str {
        "pdf_to_images"
    }

    fn description(&self) -> &str {
        "Render PDF pages to PNG images so you can SEE them with vision. \
         Use this when a PDF contains figures, diagrams, screenshots, charts, \
         tables-as-images, signatures, or scanned content that `parse_document` \
         (text only) cannot convey — e.g. you parsed a PDF's text but the user \
         is asking about something visual in it. Pass `page_range` (e.g. \"3-5\") \
         to render only the pages you need. Returns a list of page-image paths; \
         then call `analyze_image(image='<path>', question='...')` ONE PAGE AT A \
         TIME to view each (do not bundle pages — providers cap request size)."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the PDF file"
                },
                "pages": {
                    "type": "array",
                    "items": {"type": "integer", "minimum": 1},
                    "description": "Optional: specific page numbers to render (1-indexed). Prefer `page_range` for spans."
                },
                "page_range": {
                    "type": "string",
                    "description": "Optional: page range like \"1-5\", \"3,7,10-12\". Merged with `pages`. Omit to render from page 1 up to the cap."
                }
            },
            "required": ["path"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadFiles]
    }

    fn requires_approval(&self) -> bool {
        false
    }

    fn validate_input(&self, input: &Value) -> Result<()> {
        let _: PdfToImagesInput = serde_json::from_value(input.clone())
            .map_err(|e| ToolError::InvalidInput(format!("Invalid input: {}", e)))?;
        Ok(())
    }

    async fn execute(&self, input: Value, context: &ToolExecutionContext) -> Result<ToolResult> {
        let input: PdfToImagesInput = serde_json::from_value(input)?;

        let path = super::error::resolve_tool_path(&input.path, &context.working_dir());
        if !path.exists() {
            return Ok(ToolResult::error(format!(
                "File not found: {}",
                path.display()
            )));
        }
        if path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            != Some("pdf".to_string())
        {
            return Ok(ToolResult::error(
                "pdf_to_images only renders .pdf files. For other formats use parse_document."
                    .to_string(),
            ));
        }

        // Merge `pages` + `page_range` into a sorted, deduped 1-indexed list.
        let mut requested: Vec<usize> = input.pages.clone().unwrap_or_default();
        if let Some(ref spec) = input.page_range {
            requested.extend(super::doc_parser::parse_page_range(spec));
        }
        requested.sort_unstable();
        requested.dedup();

        // The renderer always renders pages 1..=N from the front, returning
        // paths in page order. To honour a range we render up to the highest
        // requested page (capped), then pick the requested ones by index —
        // robust across the pdfium/pdftoppm filename schemes.
        let render_upto = match requested.last() {
            Some(&hi) => hi.min(MAX_PAGES),
            None => MAX_PAGES,
        };

        let out_dir = render_output_dir(&path);
        let path_owned = path.clone();
        let out_owned = out_dir.clone();
        let rendered = tokio::task::spawn_blocking(move || {
            crate::utils::pdf_vision::render_pdf_pages(
                path_owned.to_str().unwrap_or(""),
                render_upto,
                out_owned.to_str().unwrap_or(""),
            )
        })
        .await
        .map_err(|e| ToolError::Execution(format!("PDF render task failed: {e}")))?;

        let all_pages = match rendered {
            Ok(p) if !p.is_empty() => p,
            Ok(_) => {
                return Ok(ToolResult::error(format!(
                    "PDF renderer produced no pages for {}",
                    path.display()
                )));
            }
            Err(e) => {
                return Ok(ToolResult::error(format!(
                    "Failed to render PDF pages: {e}. Install poppler-utils (pdftoppm) or enable the 'pdfium' feature."
                )));
            }
        };

        // Select the requested pages by 1-indexed position; an empty
        // request means "all rendered pages". Out-of-range numbers are
        // reported so the model knows the document was shorter.
        let total_rendered = all_pages.len();
        let mut selected: Vec<(usize, String)> = Vec::new();
        let mut missing: Vec<usize> = Vec::new();
        if requested.is_empty() {
            for (i, p) in all_pages.iter().enumerate() {
                selected.push((i + 1, p.to_string_lossy().to_string()));
            }
        } else {
            for page in requested {
                match all_pages.get(page - 1) {
                    Some(p) => selected.push((page, p.to_string_lossy().to_string())),
                    None => missing.push(page),
                }
            }
        }

        if selected.is_empty() {
            return Ok(ToolResult::error(format!(
                "None of the requested pages exist (document rendered {total_rendered} page(s))."
            )));
        }

        let path_list: String = selected
            .iter()
            .map(|(n, p)| format!("- Page {n}: {p}"))
            .collect::<Vec<_>>()
            .join("\n");

        let mut output = format!(
            "Rendered {} page(s) of {} as images. Call `analyze_image(image='<path>', \
             question='...')` ONE PAGE AT A TIME to view each — do NOT bundle pages, \
             providers cap request body size.\n{path_list}",
            selected.len(),
            path.display(),
        );
        if !missing.is_empty() {
            output.push_str(&format!(
                "\n[Requested pages not present (document has {total_rendered} page(s)): {missing:?}]"
            ));
        }

        Ok(ToolResult::success(output)
            .with_metadata("path".to_string(), path.display().to_string())
            .with_metadata("pages_rendered".to_string(), selected.len().to_string()))
    }
}

/// Directory to render page PNGs into: a sibling `<stem>_pages` folder
/// next to the PDF, keeping renders grouped and easy to clean up.
fn render_output_dir(pdf: &Path) -> std::path::PathBuf {
    let stem = pdf
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("document");
    let parent = pdf.parent().unwrap_or_else(|| Path::new("."));
    parent.join(format!("{stem}_pages"))
}
