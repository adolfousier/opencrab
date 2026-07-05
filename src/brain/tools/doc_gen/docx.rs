//! Native DOCX backend for `generate_document` (#357).
//!
//! Pure writer: parsed block specs go in, a .docx file comes out via
//! `docx-rs`. Headings use real Word paragraph styles (so the navigation
//! pane works), lists use real numbering definitions (so bullets and
//! numbers survive editing), and tables land as proper Word tables.

use docx_rs::{
    AbstractNumbering, Docx, Footer, Header, IndentLevel, Level, LevelJc, LevelText, NumberFormat,
    Numbering, NumberingId, Paragraph, Run, Shading, Start, Style, StyleType, Table, TableCell,
    TableRow,
};
use serde::Deserialize;
use serde_json::Value;
use std::path::Path;

/// One content block of the document, in order.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum BlockSpec {
    /// Section heading. `level` 1-3 maps to Word's Heading 1-3 styles.
    Heading {
        text: String,
        #[serde(default = "default_heading_level")]
        level: u8,
    },
    /// Body paragraph.
    Paragraph {
        text: String,
        #[serde(default)]
        bold: bool,
    },
    /// Bulleted (default) or numbered list.
    List {
        items: Vec<String>,
        #[serde(default)]
        ordered: bool,
    },
    /// Table from rows of cells; first row bolded as header by default.
    Table {
        rows: Vec<Vec<Value>>,
        #[serde(default = "default_true")]
        header_bold: bool,
    },
}

/// Optional document styling (#365). Defaults keep the plain look.
#[derive(Debug, Default, serde::Deserialize)]
pub(crate) struct DocxStyle {
    /// Hex color ("#0A84FF") applied to the heading styles (the real Word
    /// styles, so the navigation pane keeps working).
    pub accent_color: Option<String>,
    /// Text rendered in the page header of every page.
    pub page_header: Option<String>,
    /// Text rendered in the page footer of every page.
    pub page_footer: Option<String>,
    /// Hex fill behind table header rows.
    pub table_header_fill: Option<String>,
    /// Alternating light fills behind table data rows.
    #[serde(default)]
    pub zebra_rows: bool,
}

/// Word color values are hex WITHOUT the leading '#'. Returns None for
/// anything that is not exactly 6 hex digits, so a bad color degrades to
/// the default look instead of producing an invalid document.
fn word_hex(hex: &str) -> Option<String> {
    let h = hex.trim().trim_start_matches('#');
    if h.len() == 6 && h.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(h.to_uppercase())
    } else {
        None
    }
}

fn default_heading_level() -> u8 {
    1
}

fn default_true() -> bool {
    true
}

/// Numbering definition ids registered in every generated document.
const BULLET_NUM_ID: usize = 10;
const ORDERED_NUM_ID: usize = 11;

/// Heading style for a clamped level: (style id, display name, half-point size).
fn heading_style(level: u8) -> (&'static str, &'static str, usize) {
    match level {
        1 => ("Heading1", "Heading 1", 48),
        2 => ("Heading2", "Heading 2", 36),
        _ => ("Heading3", "Heading 3", 28),
    }
}

/// Render a JSON table cell as text: strings stay as-is, everything else
/// uses its JSON representation so no data is silently dropped.
fn cell_text(cell: &Value) -> String {
    match cell {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Write `blocks` to a .docx document at `path`. Returns a short human
/// summary for the tool result.
pub(crate) fn write_document(
    path: &Path,
    blocks: &[BlockSpec],
    style: &DocxStyle,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let mut docx = Docx::new();
    let accent = style.accent_color.as_deref().and_then(word_hex);
    let header_fill = style.table_header_fill.as_deref().and_then(word_hex);

    // Word paragraph styles for headings: real styles (not just big bold
    // runs) so the document outline and navigation pane work. Accent color
    // applies to the styles themselves, so the outline stays intact.
    for level in 1..=3u8 {
        let (id, name, size) = heading_style(level);
        let mut st = Style::new(id, StyleType::Paragraph)
            .name(name)
            .size(size)
            .bold();
        if let Some(ref color) = accent {
            st = st.color(color.clone());
        }
        docx = docx.add_style(st);
    }

    // Page furniture: header/footer text on every page.
    if let Some(text) = style.page_header.as_deref() {
        docx = docx.header(
            Header::new().add_paragraph(Paragraph::new().add_run(Run::new().add_text(text))),
        );
    }
    if let Some(text) = style.page_footer.as_deref() {
        docx = docx.footer(
            Footer::new().add_paragraph(Paragraph::new().add_run(Run::new().add_text(text))),
        );
    }
    // Real numbering definitions so list markers survive user edits.
    docx = docx
        .add_abstract_numbering(AbstractNumbering::new(BULLET_NUM_ID).add_level(Level::new(
            0,
            Start::new(1),
            NumberFormat::new("bullet"),
            LevelText::new("•"),
            LevelJc::new("left"),
        )))
        .add_numbering(Numbering::new(BULLET_NUM_ID, BULLET_NUM_ID))
        .add_abstract_numbering(AbstractNumbering::new(ORDERED_NUM_ID).add_level(Level::new(
            0,
            Start::new(1),
            NumberFormat::new("decimal"),
            LevelText::new("%1."),
            LevelJc::new("left"),
        )))
        .add_numbering(Numbering::new(ORDERED_NUM_ID, ORDERED_NUM_ID));

    let (mut headings, mut paragraphs, mut lists, mut tables) = (0usize, 0usize, 0usize, 0usize);
    for block in blocks {
        match block {
            BlockSpec::Heading { text, level } => {
                let (style_id, _, _) = heading_style((*level).clamp(1, 3));
                docx = docx.add_paragraph(
                    Paragraph::new()
                        .add_run(Run::new().add_text(text))
                        .style(style_id),
                );
                headings += 1;
            }
            BlockSpec::Paragraph { text, bold } => {
                let mut run = Run::new().add_text(text);
                if *bold {
                    run = run.bold();
                }
                docx = docx.add_paragraph(Paragraph::new().add_run(run));
                paragraphs += 1;
            }
            BlockSpec::List { items, ordered } => {
                let num_id = if *ordered {
                    ORDERED_NUM_ID
                } else {
                    BULLET_NUM_ID
                };
                for item in items {
                    docx = docx.add_paragraph(
                        Paragraph::new()
                            .add_run(Run::new().add_text(item))
                            .numbering(NumberingId::new(num_id), IndentLevel::new(0)),
                    );
                }
                lists += 1;
            }
            BlockSpec::Table { rows, header_bold } => {
                let table_rows: Vec<TableRow> = rows
                    .iter()
                    .enumerate()
                    .map(|(r, row)| {
                        let is_header = *header_bold && r == 0;
                        let data_idx = if *header_bold { r.wrapping_sub(1) } else { r };
                        let zebra = style.zebra_rows && !is_header && data_idx % 2 == 1;
                        let cells: Vec<TableCell> = row
                            .iter()
                            .map(|cell| {
                                let mut run = Run::new().add_text(cell_text(cell));
                                if is_header {
                                    run = run.bold();
                                }
                                let mut tc =
                                    TableCell::new().add_paragraph(Paragraph::new().add_run(run));
                                if is_header && let Some(ref fill) = header_fill {
                                    tc = tc.shading(Shading::new().fill(fill.clone()));
                                } else if zebra {
                                    tc = tc.shading(Shading::new().fill("F2F2F2"));
                                }
                                tc
                            })
                            .collect();
                        TableRow::new(cells)
                    })
                    .collect();
                docx = docx.add_table(Table::new(table_rows));
                tables += 1;
            }
        }
    }

    let file = std::fs::File::create(path)?;
    docx.build().pack(file)?;
    Ok(format!(
        "{} heading(s), {} paragraph(s), {} list(s), {} table(s)",
        headings, paragraphs, lists, tables
    ))
}
