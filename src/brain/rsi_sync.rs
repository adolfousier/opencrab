//! RSI Template Sync — Upstream brain file template synchronization.
//!
//! Checks for new releases, fetches updated templates from the public repo,
//! diffs against local brain files, and appends only new sections.
//!
//! State is persisted to `~/.opencrabs/rsi/state.toml`:
//! ```toml
//! last_synced_version = "0.3.14"
//! last_sync_date = "2026-04-27T21:00:00Z"
//!
//! [files]
//! SOUL.md = "2026-04-27T21:00:00Z"
//! TOOLS.md = "2026-04-27T21:00:00Z"
//! ```
//!
//! Flow:
//! 1. Version gate — compare `last_synced_version` to `crate::VERSION`. No change = bail.
//! 2. Backup all tracked files to `rsi/backups/`.
//! 3. Fetch upstream templates from raw GitHub URLs.
//! 4. Diff: extract sections in upstream that don't exist locally.
//! 5. Merge: append new sections. Log to `rsi/improvements.md`.
//! 6. Sanity check: verify file isn't empty. If failed, restore from backup.
//! 7. Update state.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::brain::tools::brain_file_safety;

/// GitHub raw URL base for templates.
/// Raw base for the repository ROOT, not the templates directory: tracked
/// files now live in both places (brain templates under
/// `src/docs/reference/templates`, config examples at the root), so each
/// entry carries its own repo-relative path (#819).
const TEMPLATE_BASE_URL: &str = "https://raw.githubusercontent.com/adolfousier/opencrabs/main";

/// A brain file: same name locally and under the templates directory.
macro_rules! md {
    ($name:literal) => {
        TrackedTemplate {
            local: $name,
            upstream: concat!("src/docs/reference/templates/", $name),
            kind: TemplateKind::Markdown,
        }
    };
}

/// A config example: `foo.toml` locally, `foo.toml.example` at the repo root.
macro_rules! toml_example {
    ($name:literal) => {
        TrackedTemplate {
            local: $name,
            upstream: concat!($name, ".example"),
            kind: TemplateKind::Toml,
        }
    };
}

/// How a template's content is merged into the user's copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateKind {
    /// Prose: append `## ` sections the local copy lacks.
    Markdown,
    /// Config: add missing keys, never touch existing values (#819).
    Toml,
}

/// A template tracked for upstream sync.
///
/// The local name and the upstream path differ for config examples: the repo
/// ships `usage_pricing.toml.example` at its root, while the user holds
/// `usage_pricing.toml` in their home. Carrying both explicitly is what lets
/// examples be tracked at all.
#[derive(Debug, Clone, Copy)]
pub struct TrackedTemplate {
    /// Filename inside `~/.opencrabs`.
    pub local: &'static str,
    /// Path relative to the repository root.
    pub upstream: &'static str,
    pub kind: TemplateKind,
}

/// Everything tracked for upstream sync.
///
/// The `.toml.example` entries are why #816 and #817 could not reach users:
/// the examples gained pricing for two models, nothing carried it into the
/// live `usage_pricing.toml`, and `/usage` reported $0.00 on real spend.
const TRACKED: &[TrackedTemplate] = &[
    // Brain files — prose, merged by section, shipped under the templates dir.
    md!("SOUL.md"),
    md!("USER.md"),
    md!("AGENTS.md"),
    md!("TOOLS.md"),
    md!("CODE.md"),
    md!("SECURITY.md"),
    md!("MEMORY.md"),
    md!("BOOT.md"),
    md!("HEARTBEAT.md"),
    // Config examples — merged by key, additively, shipped at the repo root
    // with a `.example` suffix the local copy does not carry.
    toml_example!("usage_pricing.toml"),
    toml_example!("config.toml"),
    toml_example!("commands.toml"),
    toml_example!("tools.toml"),
    toml_example!("rtk_filters.toml"),
    // keys.toml is deliberately NOT tracked: it holds credentials and the
    // upstream example carries only placeholders, so merging it would add
    // dummy keys to a working install.
];

/// Parsed state from `rsi/state.toml`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SyncState {
    pub last_synced_version: String,
    pub last_sync_date: String,
    pub file_dates: HashMap<String, String>,
}

impl SyncState {
    /// Load state from `~/.opencrabs/rsi/state.toml`.
    pub fn load() -> Self {
        let path = Self::state_path();
        if !path.exists() {
            return Self::default();
        }
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("RSI sync: failed to read state.toml: {e}");
                return Self::default();
            }
        };

        let mut state = Self::default();
        let mut in_files_section = false;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if trimmed == "[files]" {
                in_files_section = true;
                continue;
            }
            if trimmed.starts_with('[') {
                in_files_section = false;
                continue;
            }

            if let Some((key, value)) = trimmed.split_once('=') {
                let key = key.trim();
                let value = value.trim().trim_matches('"');
                if in_files_section {
                    state.file_dates.insert(key.to_string(), value.to_string());
                } else if key == "last_synced_version" {
                    state.last_synced_version = value.to_string();
                } else if key == "last_sync_date" {
                    state.last_sync_date = value.to_string();
                }
            }
        }

        state
    }

    /// Save state to `~/.opencrabs/rsi/state.toml`.
    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::state_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut content = format!(
            "last_synced_version = \"{}\"\nlast_sync_date = \"{}\"\n\n[files]\n",
            self.last_synced_version, self.last_sync_date
        );

        for (file, date) in &self.file_dates {
            content.push_str(&format!("{file} = \"{date}\"\n"));
        }

        std::fs::write(&path, content)
    }

    fn state_path() -> PathBuf {
        crate::config::opencrabs_home().join("rsi/state.toml")
    }
}

/// Result of a single file sync attempt.
#[derive(Debug, Clone, Default)]
pub struct FileSyncResult {
    pub filename: String,
    pub synced: bool,
    pub sections_added: usize,
    pub error: Option<String>,
    /// `Some(report)` when the sync bailed because the merged content
    /// would exceed `[brain.caps] <filename>` (or `default_cap`).
    /// `synced=false` in that case too, but `bailed_for_cap` distinguishes
    /// "cap reached, user must act" from "transient error, will retry".
    /// Issue #164 fix 2.
    pub bailed_for_cap: Option<CapBailReport>,
}

/// Diagnostic surfaced when `sync_single_file` refuses to write because
/// the merged content would exceed the configured per-file line cap. The
/// user sees this via tracing + an entry appended to
/// `~/.opencrabs/rsi/improvements.md` so they can either raise the cap,
/// prune the file, or add the offending sections to the pruned sidecar.
#[derive(Debug, Clone, Default)]
pub struct CapBailReport {
    pub filename: String,
    pub local_lines: usize,
    pub upstream_lines: usize,
    pub merged_lines: usize,
    pub cap: usize,
    /// Up to 3 largest new sections (`## Header (N lines)`) that the
    /// sync would have added. Helps the user judge whether to raise the
    /// cap or prune those headers specifically.
    pub top_new_sections: Vec<String>,
}

/// Check if a version change requires a sync.
pub fn needs_sync(state: &SyncState) -> bool {
    state.last_synced_version != crate::VERSION
}

/// Fetch a single template. `path` is relative to the repository root.
pub async fn fetch_template(path: &str) -> Result<String, String> {
    let filename = path;
    let url = format!("{TEMPLATE_BASE_URL}/{path}");
    let response = reqwest::get(&url)
        .await
        .map_err(|e| format!("Failed to fetch {filename}: {e}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "Failed to fetch {filename}: HTTP {}",
            response.status()
        ));
    }

    response
        .text()
        .await
        .map_err(|e| format!("Failed to read {filename} body: {e}"))
}

/// Extract sections from upstream that don't exist in local content.
///
/// Strategy: append-only, never overwrite user customizations.
///
/// Two levels of diff:
/// 1. New top-level sections (## Header) that don't exist locally → append entire section
/// 2. New subsections (### Header) under existing top-level sections → append just the subsection
///
/// This ensures user's personalized content under any header is preserved,
/// while still catching new upstream additions at both heading levels.
///
/// Returns the new sections as a string ready to append.
pub fn extract_new_sections(local: &str, upstream: &str) -> String {
    let local_headers: std::collections::HashSet<String> =
        extract_section_headers(local).into_iter().collect();

    // Parse upstream into (header_level, header_line, content_lines) blocks
    let mut blocks: Vec<(usize, String, Vec<String>)> = Vec::new();
    let mut current_level = 0;
    let mut current_header = String::new();
    let mut current_content = Vec::new();

    for line in upstream.lines() {
        let level = if line.starts_with("## ") {
            2
        } else if line.starts_with("### ") {
            3
        } else {
            0
        };

        if level >= 2 {
            // Flush previous block
            if !current_header.is_empty() {
                blocks.push((
                    current_level,
                    current_header.clone(),
                    current_content.clone(),
                ));
            }
            current_level = level;
            current_header = line.to_string();
            current_content = vec![line.to_string()];
        } else if !current_header.is_empty() {
            current_content.push(line.to_string());
        }
    }
    // Flush last block
    if !current_header.is_empty() {
        blocks.push((current_level, current_header, current_content));
    }

    let mut new_sections = Vec::new();

    for (level, header, content) in &blocks {
        if *level == 2 {
            // Top-level section: if header doesn't exist locally, include entire section
            if !local_headers.contains(header) {
                new_sections.push(content.join("\n"));
            }
        } else if *level == 3 {
            // Subsection: if this ### header doesn't exist locally, include it
            // (even if its parent ## section exists locally)
            if !local_headers.contains(header) {
                new_sections.push(content.join("\n"));
            }
        }
    }

    if new_sections.is_empty() {
        String::new()
    } else {
        format!("\n{}\n", new_sections.join("\n\n"))
    }
}

/// Extract all ## and ### heading lines from markdown.
pub(crate) fn extract_section_headers(content: &str) -> Vec<String> {
    content
        .lines()
        .filter(|line| line.starts_with("## ") || line.starts_with("### "))
        .map(|line| line.to_string())
        .collect()
}

/// Backup directory for RSI sync.
/// Merge an upstream config example into the user's live file (#819).
///
/// Additive only: keys the local file lacks are added, values it already has
/// are never touched, because those may be deliberate customisations. This is
/// what carries new model pricing (#816, #817) into a live install without
/// resetting rates the user set themselves.
fn sync_toml_file(
    local_path: &Path,
    filename: &str,
    local_content: &str,
    upstream_content: &str,
) -> FileSyncResult {
    let (merged, report) =
        match crate::brain::toml_merge::merge_additive(local_content, upstream_content) {
            Ok(v) => v,
            Err(e) => {
                // A malformed file on either side leaves the local one untouched.
                // Rewriting a working config from a broken template would be worse
                // than skipping the update.
                return FileSyncResult {
                    filename: filename.to_string(),
                    synced: false,
                    sections_added: 0,
                    error: Some(format!("{filename}: {e}")),
                    bailed_for_cap: None,
                };
            }
        };

    if report.is_empty() {
        tracing::debug!("RSI sync: {filename} has no new keys, skipping");
        return FileSyncResult {
            filename: filename.to_string(),
            synced: true,
            sections_added: 0,
            error: None,
            bailed_for_cap: None,
        };
    }

    // Back up before writing, matching the markdown path: a config the user
    // depends on must be recoverable if the merge turns out wrong.
    let backup = backups_dir().join(format!("{filename}.bak"));
    if let Err(e) = std::fs::write(&backup, local_content) {
        return FileSyncResult {
            filename: filename.to_string(),
            synced: false,
            sections_added: 0,
            error: Some(format!("{filename}: failed to back up before merge: {e}")),
            bailed_for_cap: None,
        };
    }

    if let Err(e) = std::fs::write(local_path, &merged) {
        return FileSyncResult {
            filename: filename.to_string(),
            synced: false,
            sections_added: 0,
            error: Some(format!("{filename}: failed to write merge: {e}")),
            bailed_for_cap: None,
        };
    }

    // Name what arrived rather than logging "updated": a config change the
    // user cannot see is a config change they cannot audit.
    tracing::info!(
        "RSI sync: {filename} gained {} key(s): {}",
        report.added.len(),
        report.added.join(", ")
    );
    log_toml_merge_to_improvements(filename, &report);

    FileSyncResult {
        filename: filename.to_string(),
        synced: true,
        sections_added: report.added.len(),
        error: None,
        bailed_for_cap: None,
    }
}

/// Record a config merge in `rsi/improvements.md`, listing the keys added.
fn log_toml_merge_to_improvements(filename: &str, report: &crate::brain::toml_merge::MergeReport) {
    let path = crate::config::opencrabs_home().join("rsi/improvements.md");
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        tracing::warn!("RSI sync: failed to create rsi dir for improvements log: {e}");
        return;
    }
    let entry = format!(
        "\n## {} — {filename} config sync\n\nAdded {} key(s) from the upstream example:\n{}\n",
        chrono::Utc::now().format("%Y-%m-%d %H:%M UTC"),
        report.added.len(),
        report
            .added
            .iter()
            .map(|k| format!("- `{k}`"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(mut f) => {
            if let Err(e) = f.write_all(entry.as_bytes()) {
                tracing::warn!("RSI sync: failed to append config merge to improvements: {e}");
            }
        }
        Err(e) => tracing::warn!("RSI sync: failed to open improvements log: {e}"),
    }
}

fn backups_dir() -> PathBuf {
    crate::config::opencrabs_home().join("rsi/backups")
}

/// Ensure backups directory exists.
fn ensure_backups_dir() -> std::io::Result<()> {
    std::fs::create_dir_all(backups_dir())
}

/// Run the full template sync.
///
/// Returns a list of per-file results.
pub async fn sync_templates() -> Vec<FileSyncResult> {
    let home = crate::config::opencrabs_home();
    let mut state = SyncState::load();

    if !needs_sync(&state) {
        tracing::info!(
            "RSI sync: no new release since last sync (v{}). Skipping.",
            state.last_synced_version
        );
        return vec![];
    }

    tracing::info!(
        "RSI sync: version changed from {} to {}. Starting template sync.",
        state.last_synced_version,
        crate::VERSION
    );

    // Ensure directories
    if let Err(e) = ensure_backups_dir() {
        tracing::warn!("RSI sync: failed to create backups dir: {e}");
        return vec![FileSyncResult {
            filename: "_setup".to_string(),
            synced: false,
            sections_added: 0,
            error: Some(format!("Failed to create backups dir: {e}")),
            bailed_for_cap: None,
        }];
    }

    let mut results = Vec::new();
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    // Recovery seed for profiles created before the
    // `seed_brain_templates` fix landed: if the home directory is
    // missing the core brain files entirely (counted as "more than
    // half of the templates are missing"), call the same template
    // seeder `create_profile` uses. This rescues old `opencrabs
    // profile create <name>` installs whose brain dir was left blank.
    // Existing files are NOT overwritten by the seeder, so a healthy
    // install is unaffected.
    seed_missing_templates_if_blank(&home);

    for tracked in TRACKED {
        let local_path = home.join(tracked.local);

        // Skip files that don't exist locally (don't create new brain files)
        if !local_path.exists() {
            tracing::debug!(
                "RSI sync: {} does not exist locally, skipping",
                tracked.local
            );
            continue;
        }

        let result = sync_single_file(&local_path, tracked, &now).await;
        if result.synced {
            state
                .file_dates
                .insert(tracked.local.to_string(), now.clone());
        }
        results.push(result);
    }

    // Update state
    state.last_synced_version = crate::VERSION.to_string();
    state.last_sync_date = now;
    if let Err(e) = state.save() {
        tracing::warn!("RSI sync: failed to save state.toml: {e}");
    }

    results
}

/// Recovery seed: if `home` is missing more than half of the core
/// brain-file templates, run `seed_brain_templates` to restore them.
/// Used by `sync_templates` to rescue profiles created before the
/// `create_profile` template-seeding fix.
///
/// The threshold (more than half missing) prevents a healthy install
/// from triggering re-seeding when only one or two non-template files
/// happen to be absent (e.g. user intentionally deleted USER.md). A
/// brand-new empty profile dir, by contrast, will have all 8 missing
/// and definitely needs seeding.
fn seed_missing_templates_if_blank(home: &std::path::Path) {
    const CORE: &[&str] = &[
        "SOUL.md",
        "USER.md",
        "AGENTS.md",
        "TOOLS.md",
        "MEMORY.md",
        "CODE.md",
        "SECURITY.md",
    ];
    let missing = CORE.iter().filter(|f| !home.join(f).exists()).count();
    if missing * 2 <= CORE.len() {
        return;
    }
    tracing::info!(
        "RSI sync: home '{}' is missing {}/{} core brain files — re-seeding from templates",
        home.display(),
        missing,
        CORE.len(),
    );
    crate::config::profile::seed_brain_templates(home);
}

/// Test re-export of `top_new_sections_by_size` so the regression tests
/// under `src/tests/` can exercise the ranking without going through the
/// async `sync_single_file` path (which needs network + disk + config).
pub fn top_new_sections_by_size_for_test(new_sections: &str, n: usize) -> Vec<String> {
    top_new_sections_by_size(new_sections, n)
}

/// Extract the top-N largest new sections (by line count) from the appended
/// content. Returns formatted strings like `"## Section Name (42 lines)"`.
/// Used by the cap-bail report so the user knows which headers dominate.
fn top_new_sections_by_size(new_sections: &str, n: usize) -> Vec<String> {
    let mut by_header: Vec<(String, usize)> = Vec::new();
    let mut current_header: Option<String> = None;
    let mut current_count: usize = 0;
    for line in new_sections.lines() {
        if line.starts_with("## ") {
            if let Some(h) = current_header.take() {
                by_header.push((h, current_count));
            }
            current_header = Some(line.to_string());
            current_count = 1;
        } else if current_header.is_some() {
            current_count += 1;
        }
    }
    if let Some(h) = current_header {
        by_header.push((h, current_count));
    }
    by_header.sort_by_key(|b| std::cmp::Reverse(b.1));
    by_header
        .into_iter()
        .take(n)
        .map(|(h, c)| format!("{h} ({c} lines)"))
        .collect()
}

/// Append a cap-bail diagnostic to `~/.opencrabs/rsi/improvements.md`
/// so the user sees it next session without having to scrape stdout.
fn log_cap_bail_to_improvements(report: &CapBailReport) {
    let home = crate::config::opencrabs_home();
    let improvements_path = home.join("rsi/improvements.md");
    if let Some(parent) = improvements_path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        tracing::warn!("RSI sync cap-bail: failed to create rsi dir for improvements log: {e}");
        return;
    }
    let top_list = if report.top_new_sections.is_empty() {
        "(none detected)".to_string()
    } else {
        report
            .top_new_sections
            .iter()
            .map(|s| format!("  - {s}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let entry = format!(
        "\n## [Bailed] Sync cap exceeded for {filename}\n\n\
         **Date:** {date}\n\
         **Cap:** {cap} lines\n\
         **Local file size:** {local} lines\n\
         **Upstream template size:** {upstream} lines\n\
         **Merged would be:** {merged} lines\n\
         **Top new sections that would have been added:**\n{top}\n\n\
         To resolve: raise `[brain.caps].{filename}` in config.toml, prune \
         the file, or add the offending headers to your `rsi/pruned.toml`.\n",
        filename = report.filename,
        date = chrono::Utc::now().format("%Y-%m-%d %H:%M UTC"),
        cap = report.cap,
        local = report.local_lines,
        upstream = report.upstream_lines,
        merged = report.merged_lines,
        top = top_list,
    );
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&improvements_path)
    {
        Ok(mut f) => {
            if let Err(e) = f.write_all(entry.as_bytes()) {
                tracing::warn!("RSI sync cap-bail: failed to append entry to improvements.md: {e}");
            }
        }
        Err(e) => {
            tracing::warn!("RSI sync cap-bail: failed to open improvements.md for append: {e}");
        }
    }
}

/// Sync a single brain file.
async fn sync_single_file(
    local_path: &Path,
    tracked: &TrackedTemplate,
    _timestamp: &str,
) -> FileSyncResult {
    let filename = tracked.local;
    // 1. Read local content
    let local_content = match std::fs::read_to_string(local_path) {
        Ok(c) => c,
        Err(e) => {
            return FileSyncResult {
                filename: filename.to_string(),
                synced: false,
                sections_added: 0,
                error: Some(format!("Failed to read local {filename}: {e}")),
                bailed_for_cap: None,
            };
        }
    };

    // 2. Fetch upstream template
    let upstream_content = match fetch_template(tracked.upstream).await {
        Ok(c) => c,
        Err(e) => {
            return FileSyncResult {
                filename: filename.to_string(),
                synced: false,
                sections_added: 0,
                error: Some(e),
                bailed_for_cap: None,
            };
        }
    };

    // 3. TOML takes a different route entirely (#819). Sections, pruning and
    // line caps are all prose concepts; a config file merges by key, and
    // appending a `## ` block to it would produce a duplicate table that stops
    // the file parsing.
    if tracked.kind == TemplateKind::Toml {
        return sync_toml_file(local_path, filename, &local_content, &upstream_content);
    }

    // 3. Extract new sections
    let new_sections = extract_new_sections(&local_content, &upstream_content);
    if new_sections.trim().is_empty() {
        tracing::info!("RSI sync: {filename} has no new sections, skipping");
        return FileSyncResult {
            filename: filename.to_string(),
            synced: true,
            sections_added: 0,
            error: None,
            bailed_for_cap: None,
        };
    }

    // 3b. Filter out sections the user has previously pruned
    let pruned_state = crate::brain::rsi_pruned::PrunedState::load();
    let new_sections =
        crate::brain::rsi_pruned::filter_pruned_sections(&new_sections, &pruned_state, filename);
    if new_sections.trim().is_empty() {
        tracing::info!("RSI sync: {filename} — all new sections were pruned by user, skipping");
        return FileSyncResult {
            filename: filename.to_string(),
            synced: true,
            sections_added: 0,
            error: None,
            bailed_for_cap: None,
        };
    }

    let sections_count = new_sections
        .lines()
        .filter(|l| l.starts_with("## "))
        .count();

    // 3c. Per-file line cap (issue #164 fix 2). Compute the merged line
    // count and BAIL if it would exceed the configured cap. The cap is
    // read from `[brain.caps] <filename>` with `[brain] default_cap` as
    // the fallback (500 by default). Bailing means no write, no append
    // to improvements.md beyond the warning entry below, and the caller
    // sees `bailed_for_cap = Some(...)` so Mission Control can surface
    // the situation distinctly from a transient I/O failure.
    let brain_cfg = crate::config::Config::current().brain.clone();
    let cap = brain_cfg.cap_for(filename);
    let merged_line_count = local_content.lines().count() + new_sections.lines().count();
    if merged_line_count > cap {
        let report = CapBailReport {
            filename: filename.to_string(),
            local_lines: local_content.lines().count(),
            upstream_lines: upstream_content.lines().count(),
            merged_lines: merged_line_count,
            cap,
            top_new_sections: top_new_sections_by_size(&new_sections, 3),
        };
        tracing::warn!(
            "RSI sync: {filename} BAILED — merged would be {merged} lines, cap is {cap}. \
             Top new sections: {top:?}. Raise [brain.caps].{filename} or prune sections.",
            merged = report.merged_lines,
            cap = report.cap,
            top = report.top_new_sections,
        );
        log_cap_bail_to_improvements(&report);
        return FileSyncResult {
            filename: filename.to_string(),
            synced: false,
            sections_added: 0,
            error: None,
            bailed_for_cap: Some(report),
        };
    }

    // 4. Backup before writing
    match brain_file_safety::backup_before_write(local_path) {
        Ok(Some(backup_path)) => {
            tracing::info!(
                "RSI sync: backed up {filename} to {}",
                backup_path.display()
            );
        }
        Ok(None) => {
            tracing::debug!("RSI sync: {filename} has no existing backup (file is new)");
        }
        Err(e) => {
            tracing::warn!("RSI sync: failed to backup {filename}: {e}");
        }
    }

    // 5. Append new sections
    let updated = format!("{}{}", local_content, new_sections);

    // Sanity check: file must not be empty
    if updated.trim().is_empty() {
        return FileSyncResult {
            filename: filename.to_string(),
            synced: false,
            sections_added: 0,
            error: Some("Sanity check failed: merged content is empty".to_string()),
            bailed_for_cap: None,
        };
    }

    if let Err(e) = std::fs::write(local_path, &updated) {
        return FileSyncResult {
            filename: filename.to_string(),
            synced: false,
            sections_added: 0,
            error: Some(format!("Failed to write {filename}: {e}")),
            bailed_for_cap: None,
        };
    }

    // 6. Log to improvements.md
    let home = crate::config::opencrabs_home();
    let improvements_path = home.join("rsi/improvements.md");
    let entry = format!(
        "\n## [Synced] Upstream template sync for {filename}\n\n\
         **Date:** {}\n\
         **Version:** {}\n\
         **Sections added:** {sections_count}\n\
         **Status:** Applied (upstream sync)\n",
        chrono::Utc::now().format("%Y-%m-%d %H:%M UTC"),
        crate::VERSION,
    );
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&improvements_path)
    {
        Ok(mut f) => {
            if let Err(e) = f.write_all(entry.as_bytes()) {
                tracing::warn!(
                    "RSI sync: failed to append synced-entry for {filename} to improvements.md: {e}"
                );
            }
        }
        Err(e) => {
            tracing::warn!(
                "RSI sync: failed to open improvements.md for synced-entry append on {filename}: {e}"
            );
        }
    }

    tracing::info!(
        "RSI sync: synced {filename} (+{sections_count} sections from upstream v{})",
        crate::VERSION
    );

    FileSyncResult {
        filename: filename.to_string(),
        synced: true,
        sections_added: sections_count,
        error: None,
        bailed_for_cap: None,
    }
}
