//! Regression tests for the glob tool's bounded walk.
//!
//! A `**/*BRAIN*CONSTITUTION*` glob once ran for 16+ minutes in production: the
//! old implementation drove the walk with the `glob` crate, which follows
//! symlinks (a loop never terminates), has no time/entry bound, and ran on the
//! async executor. The rewrite walks with a bounded, symlink-safe walker.

use crate::brain::tools::glob::GlobTool;
use crate::brain::tools::{Tool, ToolExecutionContext};
use uuid::Uuid;

fn ctx_in(dir: &std::path::Path) -> ToolExecutionContext {
    ToolExecutionContext::new(Uuid::new_v4()).with_working_directory(dir.to_path_buf())
}

#[cfg(unix)]
#[tokio::test]
async fn finds_file_in_hidden_dir_and_survives_symlink_loop() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();

    // Target lives under a HIDDEN dir (mirrors ~/.opencrabs/projects/...).
    let nested = root.join(".opencrabs/projects");
    std::fs::create_dir_all(&nested).expect("mkdir");
    std::fs::write(nested.join("BRAIN_CONSTITUTION.md"), b"# constitution").expect("write");

    // A symlink loop that would hang a symlink-following walk forever.
    std::os::unix::fs::symlink(root, root.join("loop")).expect("symlink");

    let result = GlobTool
        .execute(
            serde_json::json!({ "pattern": "**/*BRAIN*CONSTITUTION*" }),
            &ctx_in(root),
        )
        .await
        .expect("glob executes");

    let out = result.output;
    assert!(
        out.contains("BRAIN_CONSTITUTION.md"),
        "must find the target even though it lives under a hidden (.opencrabs) \
         dir — got: {out}"
    );
}

#[tokio::test]
async fn prunes_node_modules() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();

    std::fs::create_dir_all(root.join("src")).expect("mkdir src");
    std::fs::write(root.join("src/Target.md"), b"x").expect("write real");
    std::fs::create_dir_all(root.join("node_modules/pkg")).expect("mkdir nm");
    std::fs::write(root.join("node_modules/pkg/Target.md"), b"x").expect("write nm");

    let result = GlobTool
        .execute(
            serde_json::json!({ "pattern": "**/Target.md" }),
            &ctx_in(root),
        )
        .await
        .expect("glob executes");

    let out = result.output;
    assert!(out.contains("Target.md"), "must find the real file: {out}");
    assert!(
        !out.contains("node_modules"),
        "node_modules must be pruned from the walk, got: {out}"
    );
}

#[tokio::test]
async fn missing_file_returns_quickly_not_a_hang() {
    // The original 16-minute hang happened when the pattern matched nothing and
    // the walk traversed the whole tree. With the bounded walker this returns a
    // clean "no files found" without scanning forever.
    let tmp = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(tmp.path().join("a/b/c")).expect("mkdir");

    let result = GlobTool
        .execute(
            serde_json::json!({ "pattern": "**/*DEFINITELY_NOT_THERE*" }),
            &ctx_in(tmp.path()),
        )
        .await
        .expect("glob executes");

    assert!(
        result.output.contains("No files found"),
        "missing target must report no matches, got: {}",
        result.output
    );
}
