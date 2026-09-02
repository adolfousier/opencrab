//! The structured input of `generate_document`: one request shape for
//! every format, with the per-format sections defaulting to empty.

use serde::Deserialize;
use serde_json::Value;

use super::{docx, pptx, xlsx};

#[derive(Debug, Deserialize)]
pub(crate) struct GenerateDocumentInput {
    /// Output format. Currently: "xlsx".
    pub format: String,

    /// Path the generated file is written to.
    pub output: String,

    /// XLSX: worksheets to create (required when format is "xlsx").
    #[serde(default)]
    pub sheets: Vec<xlsx::SheetSpec>,

    /// DOCX/PDF: content blocks in order (required for those formats).
    #[serde(default)]
    pub blocks: Vec<docx::BlockSpec>,

    /// PDF: document title for the PDF metadata (optional; defaults to the
    /// output file stem).
    #[serde(default)]
    pub title: Option<String>,

    /// PPTX: slides to create (required when format is "pptx").
    #[serde(default)]
    pub slides: Vec<pptx::SlideSpec>,

    /// Optional visual styling, interpreted per format (PDF: brand colors,
    /// page furniture, zebra; XLSX: header fill, zebra, freeze, autofilter,
    /// tab color). Defaults to the plain look.
    #[serde(default)]
    pub style: Option<Value>,
}
