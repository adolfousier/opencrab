//! Resolve the current git branch for a working directory.
//!
//! Walks up parent directories looking for `.git/HEAD` (regular repo)
//! or `.git` as a file (worktree / submodule — points at the real
//! gitdir via `gitdir: <path>`). Parses the HEAD content as either:
//! - `ref: refs/heads/<name>` — returns `<name>` (the common branch case)
//! - `ref: refs/<other>/<name>` — returns the last path component
//! - a raw 40-char SHA — returns the first 7 chars (detached HEAD)
//!
//! Used by the TUI status bar to show `~/srv/rs/opencrabs (main)`. Read
//! per render with no cache: the HEAD file is tiny (~30 bytes) and stays
//! in the OS page cache, so a `git checkout` in another terminal shows
//! up on the very next frame.

use std::path::{Path, PathBuf};

/// Resolve the current git branch (or short detached SHA) for `cwd`,
/// or `None` when the path isn't inside a git repository or HEAD is
/// unreadable / malformed.
pub fn current_branch(cwd: &Path) -> Option<String> {
    let head_path = find_head(cwd)?;
    let raw = std::fs::read_to_string(&head_path).ok()?;
    parse_head(&raw)
}

/// Parse the contents of a `.git/HEAD` file. Split out so the
/// regression tests can exercise the parser without filesystem fixtures.
pub(crate) fn parse_head(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix("ref: refs/heads/") {
        return Some(rest.to_string());
    }
    if let Some(rest) = trimmed.strip_prefix("ref: ") {
        // Non-branch ref (e.g. refs/tags/<x>, refs/remotes/<x>). Use
        // the last path component as the label rather than the full ref.
        return rest.rsplit('/').next().map(String::from);
    }
    // Detached HEAD — raw SHA. Show short form.
    if trimmed.len() >= 7 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        return Some(trimmed[..7].to_string());
    }
    None
}

/// Walk up from `cwd` looking for a `.git` directory containing `HEAD`,
/// or a `.git` file (worktree / submodule) whose `gitdir:` line points
/// at the real gitdir. Returns the path to the resolved HEAD file.
fn find_head(cwd: &Path) -> Option<PathBuf> {
    let mut dir = cwd;
    loop {
        let git_path = dir.join(".git");
        if git_path.is_dir() {
            let head = git_path.join("HEAD");
            if head.is_file() {
                return Some(head);
            }
        } else if git_path.is_file()
            && let Ok(contents) = std::fs::read_to_string(&git_path)
            && let Some(rest) = contents.trim().strip_prefix("gitdir: ")
        {
            // Worktree / submodule: `.git` is a file containing
            // `gitdir: <path>`. Resolve and look for HEAD there.
            let gitdir = Path::new(rest);
            let resolved = if gitdir.is_absolute() {
                gitdir.to_path_buf()
            } else {
                dir.join(gitdir)
            };
            let head = resolved.join("HEAD");
            if head.is_file() {
                return Some(head);
            }
        }
        dir = dir.parent()?;
    }
}
