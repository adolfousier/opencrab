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

/// Nested legacy spellings: dotted paths under a shared parent table.
///
/// The zhipu → zai provider rename left `providers.zhipu` in every config
/// written before it, with `#[serde(alias = "zhipu")]` keeping the old
/// spelling loading. Same trap as #1116 one level deeper: a file carrying
/// both `[providers.zhipu]` and `[providers.zai]` is one field written twice
/// as far as serde is concerned, and fails with `duplicate field \`zai\`` —
/// which the write guard then turns into a denied write.
const NESTED_ALIASES: &[(&str, &str)] = &[("providers.zhipu", "providers.zai")];

/// Split a dotted alias pair into (parent path, legacy leaf, canonical leaf).
///
/// Both spellings must sit under the same parent path: nested aliases rename
/// a section within one table, they do not move it across parents. Anything
/// else is a declaration bug and yields `None`.
fn split_nested<'a>(
    legacy: &'a str,
    canonical: &'a str,
) -> Option<(Vec<&'a str>, &'a str, &'a str)> {
    let lp: Vec<&str> = legacy.split('.').collect();
    let cp: Vec<&str> = canonical.split('.').collect();
    if lp.len() < 2 || lp.len() != cp.len() {
        return None;
    }
    if lp[..lp.len() - 1] != cp[..cp.len() - 1] {
        return None;
    }
    let parents = lp[..lp.len() - 1].to_vec();
    Some((parents, *lp.last()?, *cp.last()?))
}

/// Walk a dotted parent path inside a `toml::Value` table; `None` if any
/// segment is missing or not a table. The fold never creates parents: a file
/// without `[providers]` has nothing to fold.
fn navigate_value<'a>(
    table: &'a mut toml::value::Table,
    path: &[&str],
) -> Option<&'a mut toml::value::Table> {
    let mut cur = table;
    for &p in path {
        cur = cur.get_mut(p)?.as_table_mut()?;
    }
    Some(cur)
}

/// Same walk for the `toml_edit` world used by the file rewriter.
fn navigate_edit<'a>(
    table: &'a mut toml_edit::Table,
    path: &[&str],
) -> Option<&'a mut toml_edit::Table> {
    let mut cur = table;
    for &p in path {
        cur = cur.get_mut(p)?.as_table_mut()?;
    }
    Some(cur)
}

/// Fold `legacy_key` into `canonical_key` within one parent table
/// (`toml::Value` world). `label` is what gets reported — the dotted path for
/// nested aliases, the bare key for top-level ones.
fn fold_in_value_table(
    table: &mut toml::value::Table,
    legacy_key: &str,
    canonical_key: &str,
    label: &'static str,
    folded: &mut Vec<&'static str>,
) {
    let Some(legacy_val) = table.remove(legacy_key) else {
        return;
    };
    folded.push(label);
    match table.get_mut(canonical_key) {
        // Both present: merge, canonical wins per key.
        Some(canon_val) => merge_into(canon_val, legacy_val),
        // Only the legacy spelling: rename it.
        None => {
            table.insert(canonical_key.to_string(), legacy_val);
        }
    }
}

/// `toml_edit` twin of [`fold_in_value_table`], preserving comments and
/// formatting: the legacy table is moved as-is, per-key merges copy items
/// verbatim.
fn fold_in_edit_table(
    table: &mut toml_edit::Table,
    legacy_key: &str,
    canonical_key: &str,
    label: &'static str,
    renamed: &mut Vec<&'static str>,
) {
    let Some(legacy_item) = table.remove(legacy_key) else {
        return;
    };
    renamed.push(label);
    match table.get_mut(canonical_key) {
        // Both present: fold the legacy keys in, canonical winning, so
        // nothing the user wrote under either name is lost.
        Some(existing) => {
            if let (Some(into), Some(from)) = (existing.as_table_mut(), legacy_item.as_table()) {
                for (k, v) in from.iter() {
                    if !into.contains_key(k) {
                        into.insert(k, v.clone());
                    }
                }
            }
        }
        // The ordinary case: a straight rename, contents and comments kept.
        None => {
            table.insert(canonical_key, legacy_item);
        }
    }
}

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
        fold_in_value_table(table, legacy, canonical, legacy, &mut folded);
    }
    for (legacy, canonical) in NESTED_ALIASES {
        let Some((parents, legacy_leaf, canonical_leaf)) = split_nested(legacy, canonical) else {
            continue;
        };
        let Some(parent) = navigate_value(table, &parents) else {
            continue;
        };
        fold_in_value_table(parent, legacy_leaf, canonical_leaf, legacy, &mut folded);
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

/// Rewrite a config file so the legacy section carries its current name.
///
/// The read-time fold above keeps both spellings working, but it leaves the
/// file untouched, so a config written years ago keeps its old section name
/// forever and the two names persist in the wild. This converges them: after
/// one run nobody has the legacy spelling, and the alias becomes dead weight
/// that can eventually be deleted.
///
/// Uses `toml_edit` rather than a `toml::Value` round-trip because the latter
/// discards comments, and these files are almost entirely comments — rewriting
/// one would cost the user every note they had written in it.
///
/// Returns the sections that were renamed, empty if the file already used the
/// current names. Only writes when something actually changed.
pub(crate) fn migrate_file(path: &std::path::Path) -> std::io::Result<Vec<&'static str>> {
    let contents = std::fs::read_to_string(path)?;
    let mut doc = match contents.parse::<toml_edit::DocumentMut>() {
        Ok(doc) => doc,
        // A file we cannot parse is not ours to rewrite. The loader reports
        // the parse error; silently mangling it here would be worse.
        Err(_) => return Ok(Vec::new()),
    };

    let mut renamed = Vec::new();
    for (legacy, canonical) in ALIASES {
        fold_in_edit_table(doc.as_table_mut(), legacy, canonical, legacy, &mut renamed);
    }
    for (legacy, canonical) in NESTED_ALIASES {
        let Some((parents, legacy_leaf, canonical_leaf)) = split_nested(legacy, canonical) else {
            continue;
        };
        let Some(parent) = navigate_edit(doc.as_table_mut(), &parents) else {
            continue;
        };
        fold_in_edit_table(parent, legacy_leaf, canonical_leaf, legacy, &mut renamed);
    }

    if !renamed.is_empty() {
        // Wholesale rewrite: say so, with the keys, like the per-key writer
        // does (#1399). A config change nobody can find in the log is a
        // config change nobody can audit.
        tracing::info!(
            "alias_merge: rewrote {} to rename legacy sections: {}",
            path.display(),
            renamed.join(", ")
        );
        std::fs::write(path, doc.to_string())?;
    }
    Ok(renamed)
}
