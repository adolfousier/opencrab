//! Native PDF backend for `generate_document` (#357).
//!
//! Pure writer: the same block specs as the DOCX backend go in, an A4 PDF
//! comes out via `printpdf` using built-in PDF fonts (Helvetica), so no
//! font files or host dependencies are needed. Layout is a simple top-down
//! flow with word wrapping and automatic page breaks.

use super::docx::BlockSpec;
use printpdf::{
    BuiltinFont, Mm, Op, PdfDocument, PdfFontHandle, PdfPage, PdfSaveOptions, Point, Pt, TextItem,
};
use serde_json::Value;
use std::path::Path;

const PAGE_W_MM: f32 = 210.0;
const PAGE_H_MM: f32 = 297.0;
const MARGIN_MM: f32 = 20.0;
/// 1 pt = 0.352778 mm.
const PT_TO_MM: f32 = 0.352_778;
/// Average Helvetica glyph width as a fraction of the font size. Slightly
/// conservative so estimated lines err on wrapping early, never overflowing
/// the right margin.
const AVG_CHAR_WIDTH: f32 = 0.55;

/// One laid-out line ready for emission.
struct Line {
    text: String,
    bold: bool,
    size: f32,
    indent_mm: f32,
    /// Extra vertical gap (mm) before this line.
    gap_before_mm: f32,
}

/// Greedy word wrap by estimated glyph width. `width_mm` is the available
/// horizontal space for the text itself.
fn wrap(text: &str, size_pt: f32, width_mm: f32) -> Vec<String> {
    let max_chars = ((width_mm / PT_TO_MM) / (size_pt * AVG_CHAR_WIDTH)).max(8.0) as usize;
    let mut lines = Vec::new();
    for raw_line in text.split('\n') {
        let mut current = String::new();
        for word in raw_line.split_whitespace() {
            let candidate_len = if current.is_empty() {
                word.chars().count()
            } else {
                current.chars().count() + 1 + word.chars().count()
            };
            if candidate_len > max_chars && !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        }
        lines.push(current);
    }
    lines
}

fn cell_text(cell: &Value) -> String {
    match cell {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn heading_size(level: u8) -> f32 {
    match level {
        1 => 20.0,
        2 => 16.0,
        _ => 13.0,
    }
}

/// Flatten blocks into positioned lines (pure layout, no PDF objects yet).
fn layout(blocks: &[BlockSpec]) -> Vec<Line> {
    let body_width = PAGE_W_MM - 2.0 * MARGIN_MM;
    let mut lines: Vec<Line> = Vec::new();
    for block in blocks {
        match block {
            BlockSpec::Heading { text, level } => {
                let size = heading_size((*level).clamp(1, 3));
                for (i, l) in wrap(text, size, body_width).into_iter().enumerate() {
                    lines.push(Line {
                        text: l,
                        bold: true,
                        size,
                        indent_mm: 0.0,
                        gap_before_mm: if i == 0 { 4.0 } else { 0.0 },
                    });
                }
            }
            BlockSpec::Paragraph { text, bold } => {
                for (i, l) in wrap(text, 11.0, body_width).into_iter().enumerate() {
                    lines.push(Line {
                        text: l,
                        bold: *bold,
                        size: 11.0,
                        indent_mm: 0.0,
                        gap_before_mm: if i == 0 { 2.0 } else { 0.0 },
                    });
                }
            }
            BlockSpec::List { items, ordered } => {
                for (n, item) in items.iter().enumerate() {
                    let marker = if *ordered {
                        format!("{}. ", n + 1)
                    } else {
                        "• ".to_string()
                    };
                    for (i, l) in wrap(item, 11.0, body_width - 6.0).into_iter().enumerate() {
                        lines.push(Line {
                            text: if i == 0 { format!("{marker}{l}") } else { l },
                            bold: false,
                            size: 11.0,
                            indent_mm: if i == 0 { 4.0 } else { 8.0 },
                            gap_before_mm: if i == 0 { 1.0 } else { 0.0 },
                        });
                    }
                }
            }
            BlockSpec::Table { rows, header_bold } => {
                let cols = rows.iter().map(Vec::len).max().unwrap_or(0).max(1);
                let col_width = body_width / cols as f32;
                for (r, row) in rows.iter().enumerate() {
                    // Wrap every cell, then emit line-by-line so multi-line
                    // cells keep the row aligned.
                    let wrapped: Vec<Vec<String>> = row
                        .iter()
                        .map(|c| wrap(&cell_text(c), 10.0, col_width - 2.0))
                        .collect();
                    let height = wrapped.iter().map(Vec::len).max().unwrap_or(1);
                    for line_idx in 0..height {
                        for (c, cell_lines) in wrapped.iter().enumerate() {
                            if let Some(text) = cell_lines.get(line_idx)
                                && !text.is_empty()
                            {
                                lines.push(Line {
                                    text: text.clone(),
                                    bold: *header_bold && r == 0,
                                    size: 10.0,
                                    indent_mm: c as f32 * col_width,
                                    // Only the first cell of the first line of
                                    // a row advances the cursor; siblings are
                                    // placed on the same baseline.
                                    gap_before_mm: if c == 0 { 0.0 } else { -1.0 },
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    lines
}

/// Write `blocks` to a PDF at `path`. Returns a short human summary.
pub(crate) fn write_pdf(
    path: &Path,
    blocks: &[BlockSpec],
    title: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let lines = layout(blocks);
    let mut pages: Vec<PdfPage> = Vec::new();
    let mut ops: Vec<Op> = Vec::new();
    let mut y_mm = PAGE_H_MM - MARGIN_MM;
    let mut page_count = 1usize;

    for line in &lines {
        let line_height_mm = line.size * PT_TO_MM * 1.35;
        let same_baseline = line.gap_before_mm < 0.0;
        if !same_baseline {
            y_mm -= line.gap_before_mm + line_height_mm;
        }
        if y_mm < MARGIN_MM {
            pages.push(PdfPage::new(
                Mm(PAGE_W_MM),
                Mm(PAGE_H_MM),
                std::mem::take(&mut ops),
            ));
            page_count += 1;
            y_mm = PAGE_H_MM - MARGIN_MM - line_height_mm;
        }
        let font = if line.bold {
            BuiltinFont::HelveticaBold
        } else {
            BuiltinFont::Helvetica
        };
        ops.extend([
            Op::StartTextSection,
            Op::SetTextCursor {
                pos: Point::new(Mm(MARGIN_MM + line.indent_mm), Mm(y_mm)),
            },
            Op::SetFont {
                font: PdfFontHandle::Builtin(font),
                size: Pt(line.size),
            },
            Op::ShowText {
                items: vec![TextItem::Text(line.text.clone())],
            },
            Op::EndTextSection,
        ]);
    }
    pages.push(PdfPage::new(Mm(PAGE_W_MM), Mm(PAGE_H_MM), ops));

    let mut warnings = Vec::new();
    let bytes = PdfDocument::new(title)
        .with_pages(pages)
        .save(&PdfSaveOptions::default(), &mut warnings);
    if !warnings.is_empty() {
        tracing::warn!(
            "generate_document: printpdf reported {} warning(s) while saving {}",
            warnings.len(),
            path.display()
        );
    }
    std::fs::write(path, bytes)?;
    Ok(format!("{} page(s), {} line(s)", page_count, lines.len()))
}
