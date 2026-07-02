//! Project directive-file discovery: tier classification, recursion, and the
//! tilde-expansion regression.
//!
//! The regression: `push_project_directives` fed `RuntimeInfo.working_directory`
//! (which arrives tilde-collapsed as `~/...`) straight into `Path::is_dir()`,
//! which never expands `~`, so the guard bailed on every real call and the
//! feature was dead code. The fix expands `~` via `expand_tilde` first. The
//! `tilde_path_is_expanded_before_scan` test locks that in.

use std::fs;
use std::path::Path;

use crate::brain::directives::{DirectiveTier, directives_mtime, discover, render};
use crate::brain::tools::error::expand_tilde;

fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, body).unwrap();
}

fn tier_of<'a>(
    files: &'a [crate::brain::directives::DirectiveFile],
    rel: &str,
) -> &'a DirectiveTier {
    &files
        .iter()
        .find(|f| f.rel_path == rel)
        .unwrap_or_else(|| panic!("expected {rel} in discovered set"))
        .tier
}

#[test]
fn root_single_files_are_always() {
    let dir = tempfile::TempDir::new().unwrap();
    write(dir.path(), "AGENTS.md", "root rules");
    write(dir.path(), ".cursorrules", "legacy cursor");
    write(dir.path(), "README.md", "not a directive");

    let files = discover(dir.path());
    let paths: Vec<&str> = files.iter().map(|f| f.rel_path.as_str()).collect();
    assert!(paths.contains(&"AGENTS.md"));
    assert!(paths.contains(&".cursorrules"));
    assert!(
        !paths.contains(&"README.md"),
        "README.md is not a directive file"
    );
    assert_eq!(tier_of(&files, "AGENTS.md"), &DirectiveTier::Always);
}

#[test]
fn nested_single_files_are_discovered() {
    let dir = tempfile::TempDir::new().unwrap();
    write(dir.path(), ".claude/CLAUDE.md", "claude project");
    write(dir.path(), ".github/copilot-instructions.md", "copilot");
    write(dir.path(), ".opencode/AGENTS.md", "opencode");

    let files = discover(dir.path());
    let paths: Vec<&str> = files.iter().map(|f| f.rel_path.as_str()).collect();
    assert!(paths.contains(&".claude/CLAUDE.md"));
    assert!(paths.contains(&".github/copilot-instructions.md"));
    assert!(paths.contains(&".opencode/AGENTS.md"));
}

#[test]
fn cursor_mdc_three_tiers() {
    let dir = tempfile::TempDir::new().unwrap();
    write(
        dir.path(),
        ".cursor/rules/always.mdc",
        "---\nalwaysApply: true\n---\nalways body",
    );
    write(
        dir.path(),
        ".cursor/rules/scoped.mdc",
        "---\nglobs: [\"src/api/**\", \"routes/**\"]\nalwaysApply: false\n---\nscoped body",
    );
    write(
        dir.path(),
        ".cursor/rules/smart.mdc",
        "---\ndescription: Apply when refactoring large functions\n---\nsmart body",
    );

    let files = discover(dir.path());
    assert_eq!(
        tier_of(&files, ".cursor/rules/always.mdc"),
        &DirectiveTier::Always
    );
    assert_eq!(
        tier_of(&files, ".cursor/rules/scoped.mdc"),
        &DirectiveTier::Conditional("src/api/**, routes/**".to_string())
    );
    assert_eq!(
        tier_of(&files, ".cursor/rules/smart.mdc"),
        &DirectiveTier::OnDemand("Apply when refactoring large functions".to_string())
    );
}

#[test]
fn cursor_block_list_globs_are_normalized() {
    let dir = tempfile::TempDir::new().unwrap();
    write(
        dir.path(),
        ".cursor/rules/block.mdc",
        "---\nglobs:\n  - src/**/*.ts\n  - test/**\n---\nbody",
    );
    let files = discover(dir.path());
    assert_eq!(
        tier_of(&files, ".cursor/rules/block.mdc"),
        &DirectiveTier::Conditional("src/**/*.ts, test/**".to_string())
    );
}

#[test]
fn claude_and_cline_rules_use_paths_frontmatter() {
    let dir = tempfile::TempDir::new().unwrap();
    write(
        dir.path(),
        ".claude/rules/testing.md",
        "no frontmatter, unconditional",
    );
    write(
        dir.path(),
        ".claude/rules/frontend.md",
        "---\npaths:\n  - src/components/**\n---\nfrontend rules",
    );
    write(dir.path(), ".clinerules/coding.md", "cline unconditional");

    let files = discover(dir.path());
    assert_eq!(
        tier_of(&files, ".claude/rules/testing.md"),
        &DirectiveTier::Always
    );
    assert_eq!(
        tier_of(&files, ".claude/rules/frontend.md"),
        &DirectiveTier::Conditional("src/components/**".to_string())
    );
    assert_eq!(
        tier_of(&files, ".clinerules/coding.md"),
        &DirectiveTier::Always
    );
}

#[test]
fn rule_dirs_are_walked_recursively() {
    let dir = tempfile::TempDir::new().unwrap();
    write(
        dir.path(),
        ".cursor/rules/backend/api.mdc",
        "---\nalwaysApply: true\n---\nnested",
    );
    let files = discover(dir.path());
    let paths: Vec<&str> = files.iter().map(|f| f.rel_path.as_str()).collect();
    assert!(
        paths.contains(&".cursor/rules/backend/api.mdc"),
        "nested .mdc under a subdirectory must be discovered"
    );
}

#[test]
fn clinerules_as_single_file_is_always() {
    let dir = tempfile::TempDir::new().unwrap();
    write(dir.path(), ".clinerules", "single-file cline rules");
    let files = discover(dir.path());
    assert_eq!(tier_of(&files, ".clinerules"), &DirectiveTier::Always);
}

#[test]
fn empty_project_discovers_nothing() {
    let dir = tempfile::TempDir::new().unwrap();
    write(dir.path(), "main.rs", "fn main() {}");
    assert!(discover(dir.path()).is_empty());
}

#[test]
fn nonexistent_root_discovers_nothing() {
    assert!(discover(Path::new("/no/such/dir/anywhere")).is_empty());
}

#[test]
fn render_emits_only_populated_tiers() {
    let dir = tempfile::TempDir::new().unwrap();
    write(dir.path(), "AGENTS.md", "x");
    write(
        dir.path(),
        ".cursor/rules/smart.mdc",
        "---\ndescription: Use for regex work\n---\nbody",
    );
    let files = discover(dir.path());
    let out = render("~/proj", &files);

    assert!(out.contains("--- Project Directive Files (in ~/proj/) ---"));
    assert!(out.contains("Always apply"));
    assert!(out.contains("AGENTS.md"));
    assert!(out.contains("On-demand"));
    assert!(out.contains("Use for regex work"));
    // No conditional files present, so that section must be omitted.
    assert!(!out.contains("Conditional"));
}

/// The core regression: a tilde-collapsed working directory must be expanded
/// before the filesystem scan. Under the old code `Path::new("~/proj").is_dir()`
/// was always false and nothing was ever discovered.
#[test]
fn tilde_path_is_expanded_before_scan() {
    let temp_home = tempfile::TempDir::new().unwrap();
    let _lock = crate::tests::HOME_ENV_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let prev_home = std::env::var_os("HOME");
    let prev_userprofile = std::env::var_os("USERPROFILE");
    // SAFETY: HOME_ENV_LOCK serializes HOME mutation for the guard's lifetime.
    unsafe {
        std::env::set_var("HOME", temp_home.path());
        std::env::set_var("USERPROFILE", temp_home.path());
    }

    write(&temp_home.path().join("proj"), "AGENTS.md", "project rules");
    let expanded = expand_tilde("~/proj");
    let files = discover(&expanded);

    unsafe {
        match prev_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        match prev_userprofile {
            Some(v) => std::env::set_var("USERPROFILE", v),
            None => std::env::remove_var("USERPROFILE"),
        }
    }

    assert!(
        files.iter().any(|f| f.rel_path == "AGENTS.md"),
        "tilde path must expand and discover the directive file"
    );
}

#[test]
fn directives_mtime_is_none_for_empty_or_missing() {
    let dir = tempfile::TempDir::new().unwrap();
    write(dir.path(), "main.rs", "fn main() {}");
    assert!(
        directives_mtime(dir.path()).is_none(),
        "a project with no directive files has no directive mtime"
    );
    assert!(directives_mtime(Path::new("/no/such/dir/anywhere")).is_none());
}

#[test]
fn directives_mtime_advances_when_a_directive_file_is_added() {
    let dir = tempfile::TempDir::new().unwrap();
    write(dir.path(), "AGENTS.md", "root rules");
    let before = directives_mtime(dir.path()).expect("AGENTS.md gives an mtime");

    // Add a second directive file with a strictly newer mtime.
    let nested = dir.path().join(".cursor/rules/api.mdc");
    fs::create_dir_all(nested.parent().unwrap()).unwrap();
    fs::write(&nested, "---\nalwaysApply: true\n---\nbody").unwrap();
    let newer = before + std::time::Duration::from_secs(120);
    fs::File::options()
        .write(true)
        .open(&nested)
        .unwrap()
        .set_modified(newer)
        .unwrap();

    let after = directives_mtime(dir.path()).expect("still has directive files");
    assert!(
        after >= newer,
        "adding a newer directive file must advance the fingerprint: {after:?} < {newer:?}"
    );
}
