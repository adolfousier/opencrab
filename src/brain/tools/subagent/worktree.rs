//! A private working tree per sub-agent.
//!
//! Children inherit the parent's directory, so a fan-out collides by
//! construction: spawn three to work on one codebase and all three write the
//! same bytes with no arbitration (#1151).
//!
//! A `git worktree` gives each child its own checkout of the same repository.
//! The cost is the source, not the build: `.git` is shared, and on this
//! codebase a checkout is ~20MB against a 39GB `target/`. That build cache is
//! the whole expense, so it is cloned copy-on-write where the filesystem
//! supports it (APFS `cp -c`, measured at 0 bytes and 0.00s for 200MB) and
//! otherwise left absent rather than duplicated.
//!
//! Nothing here is load-bearing for correctness: every failure degrades to the
//! parent's directory, which is exactly the behaviour that exists today. A
//! child that cannot get its own tree still runs.

use std::path::{Path, PathBuf};
use std::process::Command;

/// A checkout owned by one sub-agent, removed when the agent is done unless it
/// produced work worth keeping.
#[derive(Debug, Clone)]
pub(crate) struct SubagentWorktree {
    /// Where the child works.
    pub(crate) path: PathBuf,
    /// The branch created for it, `subagent/<short-id>`.
    pub(crate) branch: String,
    /// The repository it was cut from, needed to prune it later.
    repo: PathBuf,
    /// The commit the tree started at. A child has produced something exactly
    /// when HEAD has moved off this, which is answerable without a remote:
    /// asking git for commits "not on a remote" calls every commit work in a
    /// repository that has no remote, so every tree looked worth keeping and
    /// none was ever returned.
    base_commit: Option<String>,
}

/// Is `dir` inside a git repository? A worktree cannot be cut otherwise, and
/// asking git is cheaper than guessing from a `.git` entry that may be a file.
fn is_git_repo(dir: &Path) -> bool {
    Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(dir)
        .output()
        .map(|o| o.status.success() && o.stdout.starts_with(b"true"))
        .unwrap_or(false)
}

/// Where a child's tree lives: beside the repository, not inside it, so it can
/// never be picked up by the parent's own globs, builds or `git add`.
fn worktree_root() -> PathBuf {
    crate::config::profile::resolve_profile_home().join("worktrees")
}

/// Copy `target/` into the new tree without spending the disk.
///
/// `cp -c` asks APFS for a clone: the copy shares blocks until one side writes,
/// so a child starts with a warm build cache for effectively nothing. On a
/// filesystem without clonefile the command fails and the child simply builds
/// from cold, which is slow but correct. It is never worth duplicating 39GB to
/// avoid that.
fn clone_build_cache(from: &Path, to: &Path) {
    let src = from.join("target");
    if !src.is_dir() {
        return;
    }
    match Command::new("cp")
        .arg("-c")
        .arg("-R")
        .arg(&src)
        .arg(to.join("target"))
        .output()
    {
        Ok(out) if out.status.success() => {
            tracing::debug!(
                "sub-agent worktree: cloned build cache into {}",
                to.display()
            );
        }
        Ok(out) => {
            // Expected on any non-APFS filesystem. The child builds cold.
            tracing::debug!(
                "sub-agent worktree: no clonefile support, starting without a build cache: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Err(e) => tracing::debug!("sub-agent worktree: could not clone build cache: {e}"),
    }
}

/// Cut a private tree for `agent_id`, or `None` to keep using `parent_dir`.
///
/// `None` is a normal outcome, not a failure: outside a repository, or when
/// git refuses, the child runs where it always did.
pub(crate) fn create(parent_dir: &Path, agent_id: &str) -> Option<SubagentWorktree> {
    if !is_git_repo(parent_dir) {
        return None;
    }

    let branch = format!("subagent/{agent_id}");
    let path = worktree_root().join(agent_id);

    if let Err(e) = std::fs::create_dir_all(worktree_root()) {
        tracing::warn!("sub-agent worktree: cannot create root: {e}");
        return None;
    }

    // A path left by a previous run makes `git worktree add` refuse outright.
    // That happens after a crash, and it would take the isolation down for
    // every later child while the stale directory sat there. Clear the
    // registration and the directory before asking for a new one; a tree that
    // held work was kept under a branch, which survives this.
    if path.exists() {
        tracing::info!(
            "sub-agent worktree: clearing a stale tree at {}",
            path.display()
        );
        let _ = Command::new("git")
            .args(["worktree", "remove", "--force"])
            .arg(&path)
            .current_dir(parent_dir)
            .output();
        let _ = std::fs::remove_dir_all(&path);
        let _ = Command::new("git")
            .args(["worktree", "prune"])
            .current_dir(parent_dir)
            .output();
    }

    let out = Command::new("git")
        .args(["worktree", "add", "--detach"])
        .arg(&path)
        .current_dir(parent_dir)
        .output();

    match out {
        Ok(o) if o.status.success() => {}
        Ok(o) => {
            tracing::warn!(
                "sub-agent worktree: git refused, child will share the parent tree: {}",
                String::from_utf8_lossy(&o.stderr).trim()
            );
            return None;
        }
        Err(e) => {
            tracing::warn!("sub-agent worktree: could not run git: {e}");
            return None;
        }
    }

    // Branch inside the new tree so the parent's HEAD is untouched.
    let _ = Command::new("git")
        .args(["switch", "-c", &branch])
        .current_dir(&path)
        .output();

    clone_build_cache(parent_dir, &path);

    let base_commit = head_commit(&path);

    tracing::info!(
        "sub-agent worktree: {} on branch {}",
        path.display(),
        branch
    );
    Some(SubagentWorktree {
        path,
        branch,
        repo: parent_dir.to_path_buf(),
        base_commit,
    })
}

/// The commit a tree currently sits on, if it can be read.
fn head_commit(dir: &Path) -> Option<String> {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

impl SubagentWorktree {
    /// Does this tree hold anything worth keeping?
    ///
    /// Either a commit the branch carries, or an uncommitted edit. Both mean
    /// the tree must survive: discarding a child's work silently is the one
    /// outcome worse than the collisions this exists to prevent.
    pub(crate) fn has_work(&self) -> bool {
        let dirty = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&self.path)
            .output()
            .map(|o| !o.stdout.is_empty())
            .unwrap_or(true);
        if dirty {
            return true;
        }
        // Committed work is HEAD having moved off the commit the tree was cut
        // from. When the base could not be read, assume work rather than risk
        // removing a tree that holds some.
        match (&self.base_commit, head_commit(&self.path)) {
            (Some(base), Some(head)) => &head != base,
            _ => true,
        }
    }

    /// What the parent should be told about this tree, if anything.
    ///
    /// A tree that was returned needs no mention. One that was kept does: the
    /// work is real, it is not on the parent's branch, and nothing else in the
    /// result would say so. Leaving that to a log line is how isolation turns
    /// a clobbering problem into a stranded-work problem.
    pub(crate) fn parent_notice(&self, removed: bool) -> Option<String> {
        (!removed).then(|| {
            format!(
                "\n\n---\nThis agent worked in its own checkout and left changes there.\n\
                 Branch: `{}`\nPath: `{}`\n\
                 Review with `git -C {} status`, and land it with \
                 `git merge {}` from the main tree when you want it.",
                self.branch,
                self.path.display(),
                self.path.display(),
                self.branch,
            )
        })
    }

    /// Remove the tree when the child left nothing behind.
    ///
    /// A tree holding work is kept, and its branch named in the log so it can
    /// be found later. Returns whether the tree was removed.
    pub(crate) fn cleanup(&self) -> bool {
        if self.has_work() {
            tracing::info!(
                "sub-agent worktree kept: {} has work on branch {}",
                self.path.display(),
                self.branch
            );
            return false;
        }
        let removed = Command::new("git")
            .args(["worktree", "remove", "--force"])
            .arg(&self.path)
            .current_dir(&self.repo)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !removed {
            tracing::warn!(
                "sub-agent worktree: could not remove {}, leaving it for `git worktree prune`",
                self.path.display()
            );
        }
        removed
    }
}
