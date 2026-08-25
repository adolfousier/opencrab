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

/// Identify the project the session is working in, from its manifest.
///
/// `working_dir` is the session's own working directory, the same path the
/// verification commands run in, so this asks about the folder the work is
/// actually happening in rather than guessing at a project.
///
/// Walks up to find the nearest manifest, because a session is often parked in
/// a subdirectory of the project it is working on. Without that, a session in
/// `<repo>/src` recognised nothing and the task went back to being skipped,
/// which is the behaviour this exists to remove. The NEAREST manifest wins, so
/// a package inside a monorepo is verified as itself rather than as its parent.
///
/// Bounded by [`MAX_ANCESTORS`] so an unrecognised directory cannot walk to the
/// filesystem root and adopt something unrelated on the way.
pub(crate) fn detect(working_dir: &Path) -> Option<ProjectKind> {
    working_dir
        .ancestors()
        .take(MAX_ANCESTORS)
        .find_map(detect_exactly_in)
}

/// How far up to look for a manifest. Deep enough for the nested layouts that
/// occur in practice, shallow enough that a stray directory does not inherit a
/// project it has nothing to do with.
const MAX_ANCESTORS: usize = 6;

/// The manifest in exactly this directory, checked most specific first: a
/// Flutter package carries a `package.json` too, and answering Node there
/// would run the wrong runner in the repositories that matter most.
fn detect_exactly_in(working_dir: &Path) -> Option<ProjectKind> {
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
