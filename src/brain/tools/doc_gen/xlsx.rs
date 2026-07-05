//! Native XLSX backend for `generate_document` (#357).
//!
//! Pure writer: a parsed sheet spec goes in, an .xlsx file comes out via
//! `rust_xlsxwriter`. Strings starting with `=` are written as live Excel
//! formulas so the workbook recalculates when the user edits it; everything
//! else is written as the matching native cell type.

use rust_xlsxwriter::{Color, Format, Formula, Workbook, XlsxError};
use serde::Deserialize;
use serde_json::Value;
use std::path::Path;

/// Optional workbook styling (#364). Defaults keep the plain look.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct XlsxStyle {
    /// Hex fill ("#0A84FF") behind the header row.
    pub header_fill: Option<String>,
    /// Hex font color for the header row (pairs with `header_fill`).
    pub header_font_color: Option<String>,
    /// Alternating light fills behind data rows.
    #[serde(default)]
    pub zebra_rows: bool,
    /// Freeze the first row so headers stay visible while scrolling.
    #[serde(default)]
    pub freeze_header: bool,
    /// Filter dropdowns across the used range.
    #[serde(default)]
    pub autofilter: bool,
    /// Hex color for the worksheet tab.
    pub tab_color: Option<String>,
}

/// Parse "#RRGGBB" into an xlsxwriter RGB color.
fn hex_color(hex: &str) -> Option<Color> {
    let h = hex.trim().trim_start_matches('#');
    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(h, 16).ok().map(Color::RGB)
}

/// Excel number format for a named shorthand, or the string as-is (raw
/// Excel format codes pass through).
fn num_format(name: &str) -> &str {
    match name {
        "currency" => "#,##0.00",
        "percent" => "0.0%",
        "date" => "yyyy-mm-dd",
        "integer" => "0",
        "number" => "#,##0.00",
        raw => raw,
    }
}

/// One worksheet: name plus rows of JSON cells.
#[derive(Debug, Deserialize)]
pub(crate) struct SheetSpec {
    pub name: String,
    pub rows: Vec<Vec<Value>>,
    /// Bold the first row as a header (default: true).
    #[serde(default = "default_true")]
    pub header_bold: bool,
    /// Column widths in Excel character units, applied left to right.
    #[serde(default)]
    pub column_widths: Vec<f64>,
    /// Number format per column, left to right: "currency", "percent",
    /// "date", "integer", "number", or a raw Excel format code. Empty
    /// string skips a column.
    #[serde(default)]
    pub column_formats: Vec<String>,
}

fn default_true() -> bool {
    true
}

/// Excel worksheet names: max 31 chars, no `[ ] : * ? / \`, not empty.
/// Sanitized rather than rejected so a sloppy model-provided name still
/// produces a workbook instead of a hard error.
fn sanitize_sheet_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '[' | ']' | ':' | '*' | '?' | '/' | '\\' => '_',
            other => other,
        })
        .take(31)
        .collect();
    if cleaned.trim().is_empty() {
        "Sheet".to_string()
    } else {
        cleaned
    }
}

/// Write `sheets` to an .xlsx workbook at `path`. Returns a short human
/// summary ("2 sheet(s), 34 row(s), 5 formula(s)") for the tool result.
pub(crate) fn write_workbook(
    path: &Path,
    sheets: &[SheetSpec],
    style: &XlsxStyle,
) -> Result<String, XlsxError> {
    let mut workbook = Workbook::new();
    // Header format: bold, plus fill/font colors when styled.
    let mut header_fmt = Format::new().set_bold();
    if let Some(fill) = style.header_fill.as_deref().and_then(hex_color) {
        header_fmt = header_fmt.set_background_color(fill);
    }
    if let Some(font) = style.header_font_color.as_deref().and_then(hex_color) {
        header_fmt = header_fmt.set_font_color(font);
    }
    let zebra_fill = Color::RGB(0xF2F2F2);
    let mut total_rows = 0usize;
    let mut total_formulas = 0usize;

    for spec in sheets {
        let worksheet = workbook.add_worksheet();
        worksheet.set_name(sanitize_sheet_name(&spec.name))?;
        if let Some(tab) = style.tab_color.as_deref().and_then(hex_color) {
            worksheet.set_tab_color(tab);
        }

        for (w, width) in spec.column_widths.iter().enumerate() {
            worksheet.set_column_width(w as u16, *width)?;
        }

        // Per-column cell formats, with zebra variants: index [c][0] plain
        // row, [c][1] zebra row. Column without a format still gets a
        // zebra-only variant so striping covers every cell.
        let max_cols = spec.rows.iter().map(Vec::len).max().unwrap_or(0);
        let cell_fmts: Vec<[Format; 2]> = (0..max_cols)
            .map(|c| {
                let base = match spec.column_formats.get(c).map(String::as_str) {
                    Some(f) if !f.is_empty() => Format::new().set_num_format(num_format(f)),
                    _ => Format::new(),
                };
                let zebra = base.clone().set_background_color(zebra_fill);
                [base, zebra]
            })
            .collect();

        for (r, row) in spec.rows.iter().enumerate() {
            let r = r as u32;
            for (c, cell) in row.iter().enumerate() {
                let c = c as u16;
                let header = spec.header_bold && r == 0;
                // Zebra applies to every second DATA row (header excluded).
                let data_idx = if spec.header_bold {
                    (r as usize).wrapping_sub(1)
                } else {
                    r as usize
                };
                let zebra = style.zebra_rows && !header && data_idx % 2 == 1;
                let fmt: &Format = if header {
                    &header_fmt
                } else {
                    &cell_fmts[c as usize][usize::from(zebra)]
                };
                match cell {
                    Value::Null => {}
                    Value::String(s) if s.starts_with('=') => {
                        worksheet.write_formula_with_format(r, c, Formula::new(s), fmt)?;
                        total_formulas += 1;
                    }
                    Value::String(s) => {
                        worksheet.write_string_with_format(r, c, s, fmt)?;
                    }
                    Value::Number(n) => {
                        let v = n.as_f64().unwrap_or(0.0);
                        worksheet.write_number_with_format(r, c, v, fmt)?;
                    }
                    Value::Bool(b) => {
                        worksheet.write_boolean_with_format(r, c, *b, fmt)?;
                    }
                    // Arrays/objects have no cell type: store their JSON text
                    // rather than dropping data on the floor.
                    other => {
                        worksheet.write_string_with_format(r, c, other.to_string(), fmt)?;
                    }
                }
            }
            total_rows += 1;
        }

        // Sheet-level extras once the used range is known.
        let n_rows = spec.rows.len() as u32;
        let n_cols = spec.rows.iter().map(Vec::len).max().unwrap_or(0) as u16;
        if style.freeze_header && spec.header_bold && n_rows > 0 {
            worksheet.set_freeze_panes(1, 0)?;
        }
        if style.autofilter && spec.header_bold && n_rows > 1 && n_cols > 0 {
            worksheet.autofilter(0, 0, n_rows - 1, n_cols - 1)?;
        }
    }

    workbook.save(path)?;
    Ok(format!(
        "{} sheet(s), {} row(s), {} formula(s)",
        sheets.len(),
        total_rows,
        total_formulas
    ))
}
