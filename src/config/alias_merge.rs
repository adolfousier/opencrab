//! Fold a legacy section name into its canonical one before deserializing.
//!
//! `Config` reaches the A2A settings through `#[serde(alias = "gateway")]`, so
//! a file may spell that section either way. serde treats an alias as another
//! spelling of the same field, not as a second field, so a file carrying BOTH
//! `[gateway]` and `[a2a]` fails with `duplicate field \`a2a\`` — reported
//! against line 1, nowhere near either section (#1116).
//!
//! The load path then treated that like a syntax error and fell back to the
//! last-known-good snapshot, so the instance ran on a stale copy and every
//! later edit appeared to do nothing.
//!
//! Two sections naming one feature is the underlying problem. Until the legacy
//! spelling can be retired outright, this folds them together before serde
//! sees the document: the two tables merge into the canonical key and the file
//! parses as though it had been written with one section all along. Nobody has
//! to choose which to delete, and nothing is silently dropped.

/// Legacy spellings and the canonical key each folds into.
const ALIASES: &[(&str, &str)] = &[("gateway", "a2a")];

/// Merge every known legacy section into its canonical one, in place.
///
/// Returns the names that were folded, so the caller can say what happened
/// rather than changing the document silently.
///
/// Canonical wins on a per-key conflict: a value written under the current
/// name is the more deliberate of the two, and the legacy section is by
/// definition the older edit.
pub(crate) fn fold_legacy_sections(doc: &mut toml::Value) -> Vec<&'static str> {
    let mut folded = Vec::new();
    let Some(table) = doc.as_table_mut() else {
        return folded;
    };
    for (legacy, canonical) in ALIASES {
        let Some(legacy_val) = table.remove(*legacy) else {
            continue;
        };
        folded.push(*legacy);
        match table.get_mut(*canonical) {
            // Both present: merge, canonical wins per key.
            Some(canon_val) => merge_into(canon_val, legacy_val),
            // Only the legacy spelling: rename it.
            None => {
                table.insert((*canonical).to_string(), legacy_val);
            }
        }
    }
    folded
}

/// Deep-merge `from` into `into`, keeping whatever `into` already defines.
fn merge_into(into: &mut toml::Value, from: toml::Value) {
    let (Some(into_t), toml::Value::Table(from_t)) = (into.as_table_mut(), from) else {
        // Not both tables: the canonical value stands. A scalar under one
        // spelling and a table under the other is a malformed file, and
        // guessing which the user meant would be worse than keeping the
        // canonical one.
        return;
    };
    for (k, v) in from_t {
        match into_t.get_mut(&k) {
            Some(existing) => merge_into(existing, v),
            None => {
                into_t.insert(k, v);
            }
        }
    }
}
