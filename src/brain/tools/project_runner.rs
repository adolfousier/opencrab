//! What "run the tests" means in this repository.
//!
//! Verification commands come from `ralph_loop.toml`, and the machine-wide
//! file that governs every project without its own names cargo. On a box that
//! also holds Zig, Flutter, Python and TypeScript work, that is wrong twice
//! over: a type with no entry verifies nothing and the task is skipped, and an
//! entry that does exist runs a toolchain the project does not have.
//!
//! This detects the project from its manifest so a task can be verified with
//! the runner it actually uses. It is a fallback only: an explicit entry in
//! `ralph_loop.toml` always wins, since a project that has stated how it wants
//! to be verified has said something no marker file can contradict.

use std::path::Path;

/// A toolchain recognised by its manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProjectKind {
    Rust,
    Zig,
    Flutter,
    Node,
    Python,
    Go,
}

impl ProjectKind {
    /// The command that runs this project's tests.
    pub(crate) fn test_command(self) -> &'static str {
        match self {
            ProjectKind::Rust => "cargo test",
            ProjectKind::Zig => "zig build test",
            ProjectKind::Flutter => "flutter test",
            ProjectKind::Node => "npm test",
            ProjectKind::Python => "pytest",
            ProjectKind::Go => "go test ./...",
        }
    }

    /// The command that proves this project still builds cleanly.
    pub(crate) fn build_command(self) -> &'static str {
        match self {
            ProjectKind::Rust => "cargo clippy --all-features -- -D warnings",
            ProjectKind::Zig => "zig build",
            ProjectKind::Flutter => "flutter analyze",
            ProjectKind::Node => "npm run build",
            ProjectKind::Python => "python -m compileall -q .",
            ProjectKind::Go => "go build ./...",
        }
    }
}

/// Identify the project rooted at `working_dir` by its manifest.
///
/// Checked most specific first: a Flutter package also carries Dart files, and
/// a Rust workspace inside a Node repository should still be Rust when that is
/// where the work is happening. `None` means no manifest was recognised, and
/// the caller must not guess a runner from nothing.
pub(crate) fn detect(working_dir: &Path) -> Option<ProjectKind> {
    let has = |name: &str| working_dir.join(name).is_file();
    if has("pubspec.yaml") {
        return Some(ProjectKind::Flutter);
    }
    if has("Cargo.toml") {
        return Some(ProjectKind::Rust);
    }
    if has("build.zig") {
        return Some(ProjectKind::Zig);
    }
    if has("go.mod") {
        return Some(ProjectKind::Go);
    }
    if has("pyproject.toml") || has("setup.py") || has("setup.cfg") {
        return Some(ProjectKind::Python);
    }
    if has("package.json") {
        return Some(ProjectKind::Node);
    }
    None
}

/// Verification commands for `task_type` in this project, when the config
/// named none.
///
/// Only the two types with an unambiguous meaning are answered. Anything else
/// returns `None` and stays unverified rather than being handed a command that
/// does not describe it, which is the behaviour the config already documents.
pub(crate) fn fallback_commands(working_dir: &Path, task_type: &str) -> Option<Vec<String>> {
    let kind = detect(working_dir)?;
    match task_type.to_lowercase().as_str() {
        "test" => Some(vec![kind.test_command().to_string()]),
        "build" => Some(vec![kind.build_command().to_string()]),
        _ => None,
    }
}
