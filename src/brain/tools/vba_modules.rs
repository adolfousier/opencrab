//! VBA macro source extraction for macro-enabled workbooks (#960).
//!
//! `parse_document` dumps sheet values only, so the VBA project inside an
//! .xlsm / .xlsb / .xls stays invisible: the agent can neither review the
//! automation a workbook actually performs nor triage a suspicious macro,
//! which is the classic document-malware vector. calamine already parses the
//! CFB container behind `Reader::vba_project`, so this reads it with no new
//! dependency and no parser work.
//!
//! ODS is deliberately absent. OpenDocument uses Basic rather than VBA and
//! calamine's `Ods::vba_project` is a hardcoded `Ok(None)`, so routing it
//! here would only add a call that can never return anything.

use calamine::Reader;
use std::io::{Read, Seek};

/// Longest single module rendered. Generated form and sheet modules run long,
/// and one fat module should not crowd the rest of the project out of the
/// output.
const MAX_MODULE_BYTES: usize = 32 * 1024;

/// Ceiling across every module in one workbook, so a macro-heavy file cannot
/// flood the agent's context with source it did not ask for.
const MAX_TOTAL_BYTES: usize = 128 * 1024;

/// One decompressed VBA module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VbaModule {
    pub name: String,
    pub source: String,
}

/// Clip `source` to at most `max_bytes`, stepping back to the nearest char
/// boundary so the result is always valid UTF-8.
///
/// Returns the slice and whether anything was removed.
pub fn clip(source: &str, max_bytes: usize) -> (&str, bool) {
    if source.len() <= max_bytes {
        return (source, false);
    }
    let mut end = max_bytes;
    while end > 0 && !source.is_char_boundary(end) {
        end -= 1;
    }
    (&source[..end], true)
}

/// Render extracted modules as the `=== VBA modules ===` section appended to
/// the sheet dump. Empty input renders nothing at all, so a macro-free
/// workbook's output is byte-identical to what it was before this existed.
pub fn render(modules: &[VbaModule]) -> String {
    if modules.is_empty() {
        return String::new();
    }

    let mut out = String::from("\n=== VBA modules ===\n");
    for module in modules {
        out.push_str(&format!("- {}\n", module.name));
    }

    let mut budget = MAX_TOTAL_BYTES;
    for module in modules {
        out.push_str(&format!("\n--- Module: {} ---\n", module.name));

        if budget == 0 {
            out.push_str("[omitted: total VBA size limit reached]\n");
            continue;
        }

        let (body, clipped) = clip(&module.source, MAX_MODULE_BYTES.min(budget));
        budget = budget.saturating_sub(body.len());
        out.push_str(body);
        if !body.ends_with('\n') {
            out.push('\n');
        }
        if clipped {
            out.push_str("[truncated]\n");
        }
    }

    out
}

/// Read every module out of an already-opened workbook's VBA project.
///
/// A workbook with no macros yields an empty vec. Failures are logged and
/// skipped rather than propagated: a corrupt or password-locked VBA project
/// must not cost the caller the sheet data it actually asked for.
pub fn extract<RS, R>(workbook: &mut R) -> Vec<VbaModule>
where
    RS: Read + Seek,
    R: Reader<RS>,
{
    let project = match workbook.vba_project() {
        Ok(Some(project)) => project,
        Ok(None) => return Vec::new(),
        Err(e) => {
            tracing::warn!("VBA project unreadable, macro source skipped: {e:?}");
            return Vec::new();
        }
    };

    let mut modules = Vec::new();
    for name in project.get_module_names() {
        match project.get_module(name) {
            Ok(source) => modules.push(VbaModule {
                name: name.to_string(),
                source,
            }),
            Err(e) => {
                tracing::warn!("VBA module {name:?} did not decompress, skipped: {e:?}");
            }
        }
    }
    modules
}

/// Extract and append in one call, so the parser's format arms stay one line
/// wider than they were.
pub fn append<RS, R>(workbook: &mut R, output: &mut String)
where
    RS: Read + Seek,
    R: Reader<RS>,
{
    output.push_str(&render(&extract(workbook)));
}
