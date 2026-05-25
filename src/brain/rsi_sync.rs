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
const TEMPLATE_BASE_URL: &str =
    "https://raw.githubusercontent.com/adolfousier/opencrabs/main/src/docs/reference/templates";

/// Brain files tracked for upstream sync.
const TRACKED_FILES: &[&str] = &[
    "SOUL.md",
    "USER.md",
    "AGENTS.md",
    "TOOLS.md",
    "CODE.md",
    "SECURITY.md",
    "MEMORY.md",
    "BOOT.md",
    "BOOTSTRAP.md",
    "IDENTITY.md",
    "HEARTBEAT.md",
    "VOICE.md",
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
#[derive(Debug, Clone)]
pub struct FileSyncResult {
    pub filename: String,
    pub synced: bool,
    pub sections_added: usize,
    pub error: Option<String>,
}

/// Check if a version change requires a sync.
pub fn needs_sync(state: &SyncState) -> bool {
    state.last_synced_version != crate::VERSION
}

/// Fetch a single template from GitHub raw URL.
pub async fn fetch_template(filename: &str) -> Result<String, String> {
    let url = format!("{TEMPLATE_BASE_URL}/{filename}");
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

    for filename in TRACKED_FILES {
        let local_path = home.join(filename);

        // Skip files that don't exist locally (don't create new brain files)
        if !local_path.exists() {
            tracing::debug!("RSI sync: {filename} does not exist locally, skipping");
            continue;
        }

        let result = sync_single_file(&local_path, filename, &now).await;
        if result.synced {
            state.file_dates.insert(filename.to_string(), now.clone());
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
        "IDENTITY.md",
        "USER.md",
        "AGENTS.md",
        "TOOLS.md",
        "MEMORY.md",
        "CODE.md",
        "SECURITY.md",
    ];
    let missing = CORE
        .iter()
        .filter(|f| !home.join(f).exists())
        .count();
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

/// Sync a single brain file.
async fn sync_single_file(local_path: &Path, filename: &str, _timestamp: &str) -> FileSyncResult {
    // 1. Read local content
    let local_content = match std::fs::read_to_string(local_path) {
        Ok(c) => c,
        Err(e) => {
            return FileSyncResult {
                filename: filename.to_string(),
                synced: false,
                sections_added: 0,
                error: Some(format!("Failed to read local {filename}: {e}")),
            };
        }
    };

    // 2. Fetch upstream template
    let upstream_content = match fetch_template(filename).await {
        Ok(c) => c,
        Err(e) => {
            return FileSyncResult {
                filename: filename.to_string(),
                synced: false,
                sections_added: 0,
                error: Some(e),
            };
        }
    };

    // 3. Extract new sections
    let new_sections = extract_new_sections(&local_content, &upstream_content);
    if new_sections.trim().is_empty() {
        tracing::info!("RSI sync: {filename} has no new sections, skipping");
        return FileSyncResult {
            filename: filename.to_string(),
            synced: true,
            sections_added: 0,
            error: None,
        };
    }

    let sections_count = new_sections
        .lines()
        .filter(|l| l.starts_with("## "))
        .count();

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
        };
    }

    if let Err(e) = std::fs::write(local_path, &updated) {
        return FileSyncResult {
            filename: filename.to_string(),
            synced: false,
            sections_added: 0,
            error: Some(format!("Failed to write {filename}: {e}")),
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
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&improvements_path)
    {
        let _ = f.write_all(entry.as_bytes());
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
    }
}
