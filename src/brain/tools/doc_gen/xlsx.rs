//! Native XLSX backend for `generate_document` (#357).
//!
//! Pure writer: a parsed sheet spec goes in, an .xlsx file comes out via
//! `rust_xlsxwriter`. Strings starting with `=` are written as live Excel
//! formulas so the workbook recalculates when the user edits it; everything
//! else is written as the matching native cell type.

use rust_xlsxwriter::{Format, Formula, Workbook, XlsxError};
use serde::Deserialize;
use serde_json::Value;
use std::path::Path;

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
pub(crate) fn write_workbook(path: &Path, sheets: &[SheetSpec]) -> Result<String, XlsxError> {
    let mut workbook = Workbook::new();
    let bold = Format::new().set_bold();
    let mut total_rows = 0usize;
    let mut total_formulas = 0usize;

    for spec in sheets {
        let worksheet = workbook.add_worksheet();
        worksheet.set_name(sanitize_sheet_name(&spec.name))?;

        for (w, width) in spec.column_widths.iter().enumerate() {
            worksheet.set_column_width(w as u16, *width)?;
        }

        for (r, row) in spec.rows.iter().enumerate() {
            let r = r as u32;
            for (c, cell) in row.iter().enumerate() {
                let c = c as u16;
                let header = spec.header_bold && r == 0;
                match cell {
                    Value::Null => {}
                    Value::String(s) if s.starts_with('=') => {
                        worksheet.write_formula(r, c, Formula::new(s))?;
                        total_formulas += 1;
                    }
                    Value::String(s) => {
                        if header {
                            worksheet.write_string_with_format(r, c, s, &bold)?;
                        } else {
                            worksheet.write_string(r, c, s)?;
                        }
                    }
                    Value::Number(n) => {
                        let v = n.as_f64().unwrap_or(0.0);
                        if header {
                            worksheet.write_number_with_format(r, c, v, &bold)?;
                        } else {
                            worksheet.write_number(r, c, v)?;
                        }
                    }
                    Value::Bool(b) => {
                        worksheet.write_boolean(r, c, *b)?;
                    }
                    // Arrays/objects have no cell type: store their JSON text
                    // rather than dropping data on the floor.
                    other => {
                        worksheet.write_string(r, c, other.to_string())?;
                    }
                }
            }
            total_rows += 1;
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
