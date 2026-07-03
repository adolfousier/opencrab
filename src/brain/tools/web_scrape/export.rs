//! Persist scraped markdown to disk, profile- and project-aware.
//!
//! When a scrape (or a whole-sitemap crawl) is asked to save its output, the
//! files land where the rest of a session's artifacts live, not in some fixed
//! global path:
//!
//! 1. If the session is assigned to a **project**, markdown goes under that
//!    project's files dir (`projects/<slug>/files/scrapes/`), so a scrape sits
//!    beside everything else produced for that project.
//! 2. Otherwise it goes under the **active profile's** home in a `scrapes/`
//!    subdir (`~/.opencrabs/scrapes/` for the default profile,
//!    `~/.opencrabs/profiles/<name>/scrapes/` under `-p <name>`).
//!
//! Both resolve through the same profile-aware helpers the rest of the codebase
//! uses, so a job running under `-p ops` never writes into the default home.

use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::services::{FileService, ServiceContext};

/// Resolve the directory scraped markdown should be written to for `session_id`.
///
/// Prefers the session's project files dir when the session belongs to a
/// project; otherwise falls back to a `scrapes/` subdir of the active profile's
/// home. Both cases are namespaced under `scrapes/` so exported pages stay
/// grouped and never clutter the root of a project's or profile's files.
pub async fn resolve_export_dir(
    session_id: Uuid,
    service_context: Option<&ServiceContext>,
) -> PathBuf {
    if let Some(sc) = service_context
        && let Some(project_files) = FileService::new(sc.clone())
            .project_files_dir(session_id)
            .await
    {
        return project_files.join("scrapes");
    }
    crate::config::opencrabs_home().join("scrapes")
}

/// Derive a filesystem-safe `.md` filename from a page URL.
///
/// Uses the host plus path so files from a crawl stay distinguishable
/// (`example.com/blog/post-1` -> `example.com-blog-post-1.md`). Every run of
/// non-alphanumeric characters collapses to a single dash, leading/trailing
/// dashes are trimmed, and an empty path yields a stable `index` stem so the
/// root URL of a site still gets a sensible name.
pub fn filename_for_url(url: &str) -> String {
    let parsed = url::Url::parse(url).ok();
    let host = parsed.as_ref().and_then(|u| u.host_str()).unwrap_or("page");
    let path = parsed.as_ref().map(|u| u.path()).unwrap_or("");

    let raw = format!("{host}{path}");
    let mut slug = String::with_capacity(raw.len());
    let mut prev_dash = false;
    for ch in raw.chars() {
        // Keep alphanumerics and dots (dots keep host names like `example.com`
        // readable); collapse every other run into a single dash.
        if ch.is_ascii_alphanumeric() || ch == '.' {
            slug.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            slug.push('-');
            prev_dash = true;
        }
    }
    let stem = slug.trim_matches(|c| c == '-' || c == '.');
    let stem = if stem.is_empty() { "index" } else { stem };
    format!("{stem}.md")
}

/// Write `markdown` for `url` into `dir`, creating the directory if needed.
/// Returns the path written so the caller can report it back to the user.
pub async fn write_markdown(dir: &Path, url: &str, markdown: &str) -> std::io::Result<PathBuf> {
    tokio::fs::create_dir_all(dir).await?;
    let path = dir.join(filename_for_url(url));
    tokio::fs::write(&path, markdown).await?;
    Ok(path)
}
