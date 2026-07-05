//! Native DOCX backend for `generate_document` (#357).
//!
//! Pure writer: parsed block specs go in, a .docx file comes out via
//! `docx-rs`. Headings use real Word paragraph styles (so the navigation
//! pane works), lists use real numbering definitions (so bullets and
//! numbers survive editing), and tables land as proper Word tables.

use docx_rs::{
    AbstractNumbering, Docx, IndentLevel, Level, LevelJc, LevelText, NumberFormat, Numbering,
    NumberingId, Paragraph, Run, Start, Style, StyleType, Table, TableCell, TableRow,
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
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let mut docx = Docx::new();

    // Word paragraph styles for headings: real styles (not just big bold
    // runs) so the document outline and navigation pane work.
    for level in 1..=3u8 {
        let (id, name, size) = heading_style(level);
        docx = docx.add_style(
            Style::new(id, StyleType::Paragraph)
                .name(name)
                .size(size)
                .bold(),
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
                        let cells: Vec<TableCell> = row
                            .iter()
                            .map(|cell| {
                                let mut run = Run::new().add_text(cell_text(cell));
                                if *header_bold && r == 0 {
                                    run = run.bold();
                                }
                                TableCell::new().add_paragraph(Paragraph::new().add_run(run))
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
