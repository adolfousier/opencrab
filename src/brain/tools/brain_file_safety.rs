//! Brain-file write safety: backup + append-only enforcement.
//!
//! The 2026-04-26 RSI agent rewrote `~/.opencrabs/TOOLS.md` from 33 KB
//! down to a stub by passing the entire file as `old_content` to
//! `self_improve` action="update". Same pattern hit `MEMORY.md`. Brain
//! files are append-only by user policy: removal is allowed only when
//! it deduplicates content that already exists elsewhere in the file.
//!
//! This module is the chokepoint every brain-file mutation must go
//! through. It:
//!
//! - Snapshots the file to `<name>.YYYY-MM-DDTHHMMSS.bak` before any write
//! - Rejects writes that shrink a protected file unless the caller
//!   explicitly opts into a dedup intent and the removal really is a
//!   duplicate that survives elsewhere in the result.

use std::path::Path;

/// Brain files that follow the append-only contract. Any mutation that
/// shrinks one of these without a justified dedup must be rejected.
const PROTECTED_BRAIN_FILES: &[&str] = &[
    "SOUL.md",
    "USER.md",
    "AGENTS.md",
    "TOOLS.md",
    "CODE.md",
    "SECURITY.md",
    "MEMORY.md",
    "BOOT.md",
    "IDENTITY.md",
];

/// True if `name` (a bare file name, no directories) is one of the
/// brain files the append-only contract applies to.
pub fn is_protected_brain_file(name: &str) -> bool {
    PROTECTED_BRAIN_FILES
        .iter()
        .any(|p| p.eq_ignore_ascii_case(name))
}

/// True if `path`'s file name is a protected brain file.
pub fn is_protected_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(is_protected_brain_file)
        .unwrap_or(false)
}

/// Snapshot `path` to `<path>.YYYY-MM-DDTHHMMSS.bak` before a mutation
/// happens. No-op when `path` doesn't exist yet (nothing to back up).
/// Returns the backup path on success.
///
/// Retention policy: keeps max 5 backups per file, deletes any older than 7 days.
pub fn backup_before_write(path: &Path) -> std::io::Result<Option<std::path::PathBuf>> {
    if !path.exists() {
        return Ok(None);
    }
    let stamp = chrono::Utc::now().format("%Y-%m-%dT%H%M%S");
    let mut backup = path.as_os_str().to_owned();
    backup.push(format!(".{stamp}.bak"));
    let backup = std::path::PathBuf::from(backup);
    std::fs::copy(path, &backup)?;

    // Prune old backups: keep max 5, delete any older than 7 days
    if let Err(e) = prune_backups(path, 5, 7) {
        eprintln!(
            "Warning: failed to prune backups for {}: {}",
            path.display(),
            e
        );
    }

    Ok(Some(backup))
}

/// Delete old backups for `path`, keeping at most `max_count` and removing
/// any older than `max_age_days`. Backup files match `<path>.<timestamp>.bak`.
fn prune_backups(path: &Path, max_count: usize, max_age_days: u64) -> std::io::Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
        return Ok(());
    };

    // Find all backup files for this base path
    let mut backups: Vec<(std::path::PathBuf, chrono::DateTime<chrono::Utc>)> = Vec::new();

    for entry in std::fs::read_dir(parent)? {
        let entry = entry?;
        let entry_name = entry.file_name();
        let Some(entry_str) = entry_name.to_str() else {
            continue;
        };

        // Check if this matches our backup pattern
        if !entry_str.starts_with(file_name) || !entry_str.ends_with(".bak") {
            continue;
        }

        // Extract timestamp from filename: <base>.<YYYY-MM-DDTHHMMSS>.bak
        let without_base = &entry_str[file_name.len()..];
        let without_ext = without_base
            .trim_start_matches('.')
            .trim_end_matches(".bak");

        // Parse timestamp
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(without_ext, "%Y-%m-%dT%H%M%S") {
            let utc = dt.and_utc();
            backups.push((entry.path(), utc));
        }
    }

    // Sort by timestamp descending (newest first)
    backups.sort_by_key(|b| std::cmp::Reverse(b.1));

    let cutoff = chrono::Utc::now() - chrono::Duration::days(max_age_days as i64);

    // Delete backups beyond max_count or older than cutoff
    for (i, (backup_path, timestamp)) in backups.iter().enumerate() {
        if !(i >= max_count || *timestamp < cutoff) {
            continue;
        }
        if let Err(e) = std::fs::remove_file(backup_path) {
            eprintln!(
                "Warning: failed to delete old backup {}: {}",
                backup_path.display(),
                e
            );
        }
    }

    Ok(())
}

/// Decision returned by [`check_no_shrink`].
#[derive(Debug, PartialEq, Eq)]
pub enum ShrinkCheck {
    /// Safe to write — the new content does not shrink a protected file.
    Allowed,
    /// Reject — would remove `removed_bytes` from a protected brain file.
    /// `message` is a user-facing explanation suitable for ToolResult::error.
    Rejected { message: String },
}

/// Enforce append-only on protected brain files. `path` is the file
/// being mutated, `existing` is its current content (empty if new),
/// `updated` is what the caller wants to write.
///
/// Allows shrinking when:
/// - `cleanup_intent=true`: User-initiated cleanup with approval gate (only for write_opencrabs_file)
/// - `dedup_intent=true` AND every byte that disappeared can still be found in the result
///
/// Otherwise any byte loss on a protected file is a hard reject.
pub fn check_no_shrink(
    path: &Path,
    existing: &str,
    updated: &str,
    dedup_intent: bool,
    cleanup_intent: bool,
) -> ShrinkCheck {
    if !is_protected_path(path) {
        return ShrinkCheck::Allowed;
    }
    if updated.len() >= existing.len() {
        return ShrinkCheck::Allowed;
    }

    // User-initiated cleanup: bypass append-only restriction.
    // This is only available in write_opencrabs_file (requires_approval: true),
    // not in self_improve (autonomous RSI, no approval mechanism).
    if cleanup_intent {
        return ShrinkCheck::Allowed;
    }

    let removed_bytes = existing.len().saturating_sub(updated.len());
    let label = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("brain file");

    if dedup_intent && shrink_only_drops_duplicates(existing, updated) {
        return ShrinkCheck::Allowed;
    }

    let hint = if dedup_intent {
        " (dedup_intent was set, but the bytes removed do not all reappear in the result \
          — that's not deduplication, that's deletion)"
    } else {
        ""
    };
    ShrinkCheck::Rejected {
        message: format!(
            "Refusing to shrink protected brain file {label} by {removed_bytes} bytes. \
             Brain files are append-only — use action='apply' / operation='append' to \
             add new content. Removals are only allowed for genuine deduplication, and \
             must opt in via dedup_intent=true with a result that still contains every \
             unique line of the original.{hint}"
        ),
    }
}

/// Result of checking an append for duplicate content.
#[derive(Debug)]
pub enum AppendDedup {
    /// All content is new — append as-is.
    AllNew,
    /// Some paragraphs were duplicates — append this filtered content instead.
    /// `skipped_paragraphs` counts how many were removed.
    Filtered {
        filtered_content: String,
        skipped_paragraphs: usize,
    },
    /// Everything is already in the file — skip the append entirely.
    AllDuplicate,
}

/// Split content into paragraphs (blocks of non-empty lines separated by
/// one or more blank lines). Preserves the original text of each paragraph.
fn split_paragraphs(text: &str) -> Vec<String> {
    let mut paragraphs = Vec::new();
    let mut current = String::new();

    for line in text.lines() {
        if line.trim().is_empty() {
            if !current.is_empty() {
                paragraphs.push(current.trim_end().to_string());
                current.clear();
            }
        } else {
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(line);
        }
    }
    if !current.is_empty() {
        paragraphs.push(current.trim_end().to_string());
    }

    paragraphs
}

/// Check if a paragraph already exists in the file. Uses two strategies:
/// 1. Exact substring match (the whole paragraph appears verbatim)
/// 2. Header match: if the paragraph starts with ## or ###, check if that
///    header already exists in the file
fn paragraph_exists(paragraph: &str, existing: &str) -> bool {
    let trimmed = paragraph.trim();
    if trimmed.is_empty() {
        return true;
    }

    // Exact substring match
    if existing.contains(trimmed) {
        return true;
    }

    // Header match: if paragraph starts with ## or ###, check if header exists
    if let Some(first_line) = trimmed.lines().next() {
        let header = first_line.trim();
        if (header.starts_with("## ") || header.starts_with("### "))
            && existing.lines().any(|l| l.trim() == header)
        {
            return true;
        }
    }

    // Line-level overlap for longer paragraphs: if >70% of lines exist, consider it duplicate
    let existing_lines: std::collections::HashSet<&str> = existing.lines().map(str::trim).collect();
    let para_lines: Vec<&str> = trimmed.lines().filter(|l| !l.trim().is_empty()).collect();
    if para_lines.len() >= 3 {
        let overlap = para_lines
            .iter()
            .filter(|l| existing_lines.contains(l.trim()))
            .count();
        let ratio = overlap as f64 / para_lines.len() as f64;
        if ratio > 0.7 {
            return true;
        }
    }

    false
}

/// Analyze `new_content` against `existing` and return only the genuinely
/// new portions. Works at paragraph level to preserve structure.
///
/// This replaces the old `is_duplicate_append` boolean check. Instead of
/// blocking the entire append when overlap is detected, it extracts only
/// the new paragraphs and lets those through.
pub fn filter_duplicate_append(existing: &str, new_content: &str) -> AppendDedup {
    let new_trimmed = new_content.trim();
    if new_trimmed.is_empty() {
        return AppendDedup::AllDuplicate;
    }

    // Quick check: if the entire content is a substring, it's all duplicate
    if existing.contains(new_trimmed) {
        return AppendDedup::AllDuplicate;
    }

    let paragraphs = split_paragraphs(new_trimmed);
    if paragraphs.is_empty() {
        return AppendDedup::AllDuplicate;
    }

    let mut new_paragraphs = Vec::new();
    let mut skipped = 0;

    for para in &paragraphs {
        if paragraph_exists(para, existing) {
            skipped += 1;
        } else {
            new_paragraphs.push(para.clone());
        }
    }

    if new_paragraphs.is_empty() {
        return AppendDedup::AllDuplicate;
    }

    if skipped == 0 {
        return AppendDedup::AllNew;
    }

    AppendDedup::Filtered {
        filtered_content: new_paragraphs.join("\n\n"),
        skipped_paragraphs: skipped,
    }
}

/// Legacy alias for backward compatibility with existing tests.
/// Returns true when the entire append should be skipped.
pub fn is_duplicate_append(existing: &str, new_content: &str) -> bool {
    matches!(
        filter_duplicate_append(existing, new_content),
        AppendDedup::AllDuplicate
    )
}

/// Verifies the shrink really is a dedup: every line that was in
/// `existing` must still be present in `updated` (it's allowed to
/// appear once instead of multiple times). If any line disappears
/// completely, this isn't dedup — it's deletion.
fn shrink_only_drops_duplicates(existing: &str, updated: &str) -> bool {
    let updated_lines: std::collections::HashSet<&str> =
        updated.lines().map(str::trim_end).collect();
    for line in existing.lines() {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            continue;
        }
        if !updated_lines.contains(trimmed) {
            return false;
        }
    }
    true
}
