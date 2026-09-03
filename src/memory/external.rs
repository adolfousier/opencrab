//! External index paths (#1051) — user-configured directories outside
//! `~/.opencrabs`, indexed into the `external` collection.
//!
//! Design decisions are grilled in issue #1051 and its ADRs; the ones that
//! shape this file:
//!
//! - Documents are keyed by ABSOLUTE CANONICAL PATH. Brain/memory key by
//!   basename because their corpora are single flat dirs; external trees are
//!   not, and two `CLAUDE.md` files must not collide.
//! - The configured root is canonicalized (symlink resolved); symlinks
//!   INSIDE the tree are skipped entirely — no cycles, no escapes, no
//!   duplicate documents via aliased paths (Q8).
//! - No filesystem watcher. Discovery of added/removed files rides the
//!   periodic freshness sweep; this module just walks and reconciles (ADR-002).
//! - Nothing here is silent: missing roots, unreadable roots and skipped
//!   nested paths all surface in the returned report so callers can warn
//!   (OpenClaw lesson #1 via #1051).

use super::db::Store;
use super::{COLLECTION_EXTERNAL, external_excludes, extra_paths_config};
use std::path::PathBuf;

#[cfg(feature = "code-graph")]
use super::symbol_extractor::SymbolExtractor;

/// One configured extra path after resolution.
#[derive(Debug)]
pub(crate) struct ResolvedRoot {
    /// Canonical absolute path of the root (its own symlink resolved).
    pub root: PathBuf,
    /// Include glob, matched against root-relative paths.
    pub pattern: glob::Pattern,
}

/// What happened during resolve + walk + prune. Callers turn this into
/// warnings/status — never into silence (Q6).
#[derive(Debug, Default)]
pub(crate) struct ExternalReport {
    /// Documents newly indexed or re-indexed this pass.
    pub indexed: usize,
    /// Configured paths that do not exist.
    pub missing_roots: Vec<String>,
    /// Configured paths that exist but could not be canonicalized/read.
    pub unreadable_roots: Vec<String>,
    /// Configured paths nested inside another configured path (skipped).
    pub skipped_nested: Vec<String>,
    /// Invalid glob patterns (skipped with the default instead).
    pub bad_patterns: Vec<String>,
    /// Documents pruned because they fell out of the configured roots.
    pub pruned: usize,
    /// Total matching files on disk across all roots.
    pub on_disk: usize,
}

impl ExternalReport {
    /// Log every problem in the report. Warn for problems, info for counts.
    pub fn log(&self) {
        for p in &self.missing_roots {
            tracing::warn!("memory: extra path does not exist, not indexed: {p}");
        }
        for p in &self.unreadable_roots {
            tracing::warn!("memory: extra path unreadable, not indexed: {p}");
        }
        for p in &self.skipped_nested {
            tracing::warn!("memory: extra path nested inside another extra path, skipped: {p}");
        }
        for p in &self.bad_patterns {
            tracing::warn!("memory: invalid glob pattern in extra path, using **/*.md: {p}");
        }
        if self.indexed > 0 || self.pruned > 0 || self.on_disk > 0 {
            tracing::info!(
                "memory: external paths — {} on disk, {} indexed, {} pruned",
                self.on_disk,
                self.indexed,
                self.pruned
            );
        }
    }
}

/// Expand `~/` against the OpenCrabs home and resolve relative paths against
/// it too (NOT the session cwd — that moves with `/cd` and profiles; the
/// index must stay stable).
pub(crate) fn expand_path(raw: &str) -> PathBuf {
    let home = crate::config::opencrabs_home();
    if let Some(rest) = raw.strip_prefix("~/") {
        return home.join(rest);
    }
    let p = PathBuf::from(raw);
    if p.is_absolute() { p } else { home.join(p) }
}

/// Resolve all configured extra paths into canonical roots.
///
/// Nested roots are detected AFTER canonicalization and the nested one is
/// skipped with a warning — indexing `/a` and `/a/b` would double-index
/// everything under `/a/b`.
pub(crate) fn resolve_roots() -> (Vec<ResolvedRoot>, ExternalReport) {
    let mut report = ExternalReport::default();
    let mut resolved: Vec<(String, ResolvedRoot)> = Vec::new();

    for entry in extra_paths_config() {
        let raw = entry.path().to_string();
        let expanded = expand_path(&raw);
        let root = match std::fs::canonicalize(&expanded) {
            Ok(r) if r.is_dir() => r,
            Ok(_) => {
                // Exists but is a file, not a directory.
                report.missing_roots.push(raw);
                continue;
            }
            Err(_) => {
                if expanded.exists() {
                    report.unreadable_roots.push(raw);
                } else {
                    report.missing_roots.push(raw);
                }
                continue;
            }
        };

        let pattern = match glob::Pattern::new(entry.pattern()) {
            Ok(p) => p,
            Err(_) => {
                report.bad_patterns.push(raw.clone());
                glob::Pattern::new("**/*.md").expect("static pattern is valid")
            }
        };

        resolved.push((raw, ResolvedRoot { root, pattern }));
    }

    // Nested-path detection: skip any root whose canonical path is inside an
    // earlier root. Order of appearance wins, matching config order.
    let mut kept: Vec<ResolvedRoot> = Vec::new();
    for (raw, cand) in resolved {
        let nested = kept
            .iter()
            .any(|k| cand.root.starts_with(&k.root) && cand.root != k.root);
        if nested {
            report.skipped_nested.push(raw);
        } else {
            kept.push(cand);
        }
    }

    (kept, report)
}

/// Does an exclude pattern cover this entry? For directories we also test a
/// synthetic child so subtree patterns like `.ssh/**` exclude the dir itself.
pub(crate) fn excluded(rel: &str, name: &str, is_dir: bool, excludes: &[glob::Pattern]) -> bool {
    excludes.iter().any(|p| {
        if p.matches(name) || p.matches(rel) {
            return true;
        }
        is_dir && p.matches(&format!("{rel}/x"))
    })
}

/// Recursive walk of one root. Returns absolute paths of matching files.
///
/// Symlinks inside the tree are skipped (Q8): the root itself was already
/// canonicalized by the caller. Walking is iterative with an explicit stack,
/// no recursion depth limits to worry about.
pub(crate) fn walk_root(root: &ResolvedRoot, excludes: &[glob::Pattern]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![root.root.clone()];

    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            // Symlinks inside the tree are skipped entirely — cycles, escapes
            // and duplicate-via-alias all die here (Q8).
            if path.is_symlink() {
                continue;
            }
            let Ok(rel) = path.strip_prefix(&root.root) else {
                continue;
            };
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            let name = entry.file_name().to_string_lossy().to_string();

            if path.is_dir() {
                if !excluded(&rel_str, &name, true, excludes) {
                    stack.push(path);
                }
            } else if path.is_file()
                && !excluded(&rel_str, &name, false, excludes)
                && root.pattern.matches(&rel_str)
            {
                files.push(path);
            }
        }
    }

    files.sort();
    files
}

/// Walk all resolved roots, index matching files into COLLECTION_EXTERNAL
/// keyed by absolute path, and prune documents that fell out of the current
/// root set (deleted files AND removed config paths in one motion — this is
/// also how extra_paths config changes reconcile, Q15).
///
/// FTS only: embeddings ride the existing backfill, so search works
/// immediately and vectors come online incrementally (Q12).
pub(crate) fn reindex_external(store: &Store) -> ExternalReport {
    let (roots, mut report) = resolve_roots();
    let excludes: Vec<glob::Pattern> = external_excludes()
        .iter()
        .filter_map(|s| glob::Pattern::new(s).ok())
        .collect();

    let mut on_disk: Vec<String> = Vec::new();

    #[cfg(feature = "code-graph")]
    let mut symbol_extractor = SymbolExtractor::new().ok();

    for root in &roots {
        for path in walk_root(root, &excludes) {
            let key = path.to_string_lossy().to_string();
            on_disk.push(key.clone());
            match std::fs::read_to_string(&path) {
                Ok(body) if !body.trim().is_empty() => {
                    match super::index::index_file_sync_keyed(
                        store,
                        COLLECTION_EXTERNAL,
                        &key,
                        &body,
                    ) {
                        Ok(true) => {
                            report.indexed += 1;

                            // Extract symbols from Rust files (code-graph feature)
                            #[cfg(feature = "code-graph")]
                            if path.extension().and_then(|s| s.to_str()) == Some("rs")
                                && let Some(ref mut extractor) = symbol_extractor
                            {
                                match extractor.extract(&path, &body) {
                                    Ok((symbols, call_edges)) => {
                                        // Store symbols (excluding imports)
                                        for sym in symbols.iter().filter(|s| {
                                            s.kind != super::symbol_extractor::SymbolKind::Import
                                        }) {
                                            let _ = store.insert_symbol(
                                                &sym.name,
                                                &sym.kind.to_string(),
                                                &key,
                                                sym.start_line,
                                                sym.end_line,
                                            );
                                        }

                                        // Store call edges
                                        for edge in call_edges {
                                            let _ = store.insert_call_edge(
                                                &edge.caller,
                                                &edge.callee,
                                                &key,
                                                edge.line,
                                            );
                                        }

                                        // Store imports separately
                                        for sym in symbols.iter().filter(|s| {
                                            s.kind == super::symbol_extractor::SymbolKind::Import
                                        }) {
                                            let _ = store.insert_import(
                                                &sym.name,
                                                &key,
                                                sym.start_line,
                                            );
                                        }
                                    }
                                    Err(e) => {
                                        tracing::debug!(
                                            "code-graph: failed to extract symbols from {key}: {e}"
                                        );
                                    }
                                }
                            }
                        }
                        Ok(false) => {}
                        Err(e) => {
                            tracing::warn!("memory: failed to index external file {key}: {e}")
                        }
                    }
                }
                Ok(_) => {
                    // Empty file: drop it from the on-disk set so a stale
                    // index entry gets pruned instead of preserved.
                    on_disk.pop();
                }
                Err(e) => {
                    tracing::warn!("memory: unreadable external file {key}: {e}");
                    on_disk.pop();
                }
            }
        }
    }

    // Prune anything in the external collection that is no longer on disk
    // under a configured root.
    if let Ok(db_paths) = store.get_active_document_paths(COLLECTION_EXTERNAL) {
        for db_path in &db_paths {
            if !on_disk.contains(db_path) {
                let _ = store.deactivate_document(COLLECTION_EXTERNAL, db_path);
                report.pruned += 1;
                tracing::debug!("memory: pruned external document {db_path}");
            }
        }
    }

    report.on_disk = on_disk.len();
    report
}
