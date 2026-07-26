//! Additive key-level merge for `.toml` templates (#819).
//!
//! Brain files are prose and merge by appending sections the local copy lacks.
//! TOML cannot: appending an upstream `[providers.qwen]` block beside a local
//! one produces a duplicate key, and the file stops parsing. Doing that to
//! `usage_pricing.toml` would take the user's pricing config offline entirely.
//!
//! So upstream contributes only what is MISSING. A value the user already has
//! is never touched, because their copy may be a deliberate customisation: a
//! negotiated price, a self-hosted endpoint, a trimmed model list. Upstream
//! knows about new models; it does not know better than the user about the
//! ones they already configured.
//!
//! Motivating case: #816 and #817 added pricing for two models. Users needed
//! those rows and nothing else, while keeping every rate they had already set.

use toml_edit::{DocumentMut, Item, Table};

/// What a merge changed, for the caller's log.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MergeReport {
    /// Dotted paths added, e.g. `providers.qwen.entries`.
    pub added: Vec<String>,
}

impl MergeReport {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty()
    }
}

/// Merge `upstream` into `local`, adding only keys `local` lacks.
///
/// Returns the merged document and what was added. Formatting and comments in
/// the local file survive, which is why this uses `toml_edit` rather than a
/// value round-trip: a user's annotated config must not come back stripped.
///
/// Returns `Err` if either side fails to parse, and the caller must then leave
/// the local file untouched. A malformed upstream is never a reason to rewrite
/// a working local file.
pub fn merge_additive(local: &str, upstream: &str) -> Result<(String, MergeReport), String> {
    let mut local_doc: DocumentMut = local
        .parse()
        .map_err(|e| format!("local file is not valid TOML: {e}"))?;
    let upstream_doc: DocumentMut = upstream
        .parse()
        .map_err(|e| format!("upstream template is not valid TOML: {e}"))?;

    let mut report = MergeReport::default();
    merge_table(
        local_doc.as_table_mut(),
        upstream_doc.as_table(),
        "",
        &mut report,
    );
    Ok((local_doc.to_string(), report))
}

/// Recursively add missing keys from `up` into `loc`.
fn merge_table(loc: &mut Table, up: &Table, prefix: &str, report: &mut MergeReport) {
    for (key, up_item) in up.iter() {
        let path = if prefix.is_empty() {
            key.to_string()
        } else {
            format!("{prefix}.{key}")
        };

        match loc.get_mut(key) {
            // Present on both sides and both are tables: recurse, so a new
            // model inside an existing provider is still delivered.
            Some(Item::Table(loc_sub)) => {
                if let Item::Table(up_sub) = up_item {
                    merge_table(loc_sub, up_sub, &path, report);
                }
            }
            // Present as a value. Left alone on purpose: this is the user's
            // setting, and upstream has no business overwriting it.
            Some(_) => {}
            // Absent: this is what upstream is for.
            None => {
                loc.insert(key, up_item.clone());
                report.added.push(path);
            }
        }
    }
}
