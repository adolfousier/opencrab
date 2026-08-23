//! Regression tests for #1168 — write_file partial-view overwrite guard.
//!
//! A windowed read (`start_line`/`line_count`) must not be enough to
//! overwrite an existing file: the guard demands either a prior full read in
//! the same session or explicit `overwrite_read_confirm`.

use crate::brain::tools::read::ReadTool;
use crate::brain::tools::read_state;
use crate::brain::tools::write::WriteTool;
use crate::brain::tools::{Tool, ToolExecutionContext};
use serde_json::json;
use uuid::Uuid;

fn ctx_in(dir: &std::path::Path) -> ToolExecutionContext {
    let mut ctx = ToolExecutionContext::new(Uuid::new_v4());
    ctx.working_directory = dir.to_path_buf();
    ctx
}

async fn write_raw(
    tool: &WriteTool,
    ctx: &ToolExecutionContext,
    path: &str,
    content: &str,
) -> bool {
    tool.execute(json!({"path": path, "content": content}), ctx)
        .await
        .unwrap()
        .success
}

#[tokio::test]
async fn windowed_read_then_overwrite_requires_confirm() {
    let dir = std::env::temp_dir().join(format!("wguard_{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let ctx = ctx_in(&dir);
    let write = WriteTool;
    let read = ReadTool;

    assert!(write_raw(&write, &ctx, "f.txt", "l1\nl2\nl3\nl4\n").await);

    // Windowed read: sees only lines 1-2.
    let r = read
        .execute(
            json!({"path": "f.txt", "start_line": 1, "line_count": 2}),
            &ctx,
        )
        .await
        .unwrap();
    assert!(r.success);

    // Overwrite without confirm → guarded error naming size, read state, param.
    let w = write
        .execute(json!({"path": "f.txt", "content": "clobbered"}), &ctx)
        .await
        .unwrap();
    assert!(!w.success, "partial view must not silently clobber");
    let err = w.error.as_deref().unwrap_or(&w.output);
    assert!(
        err.contains("overwrite_read_confirm"),
        "hint missing: {}",
        err
    );
    assert!(err.contains("not fully read"), "state missing: {}", err);

    // Explicit confirm → proceeds.
    let w2 = write
        .execute(
            json!({"path": "f.txt", "content": "clobbered", "overwrite_read_confirm": true}),
            &ctx,
        )
        .await
        .unwrap();
    assert!(w2.success, "confirm must unlock the overwrite");
    assert_eq!(
        std::fs::read_to_string(dir.join("f.txt")).unwrap(),
        "clobbered"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn full_read_unlocks_overwrite_without_confirm() {
    let dir = std::env::temp_dir().join(format!("wguard_{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let ctx = ctx_in(&dir);
    let write = WriteTool;
    let read = ReadTool;

    assert!(write_raw(&write, &ctx, "g.txt", "a\nb\nc\n").await);

    // Full read (no windowing).
    let r = read.execute(json!({"path": "g.txt"}), &ctx).await.unwrap();
    assert!(r.success);

    let w = write
        .execute(json!({"path": "g.txt", "content": "rewritten"}), &ctx)
        .await
        .unwrap();
    assert!(
        w.success,
        "full read should unlock overwrite: {:?}",
        w.error
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("g.txt")).unwrap(),
        "rewritten"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn new_file_never_guarded() {
    let dir = std::env::temp_dir().join(format!("wguard_{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let ctx = ctx_in(&dir);
    let write = WriteTool;

    // Brand-new path: no prior state can exist, write goes straight through.
    assert!(write_raw(&write, &ctx, "fresh.txt", "hello").await);
    assert_eq!(
        std::fs::read_to_string(dir.join("fresh.txt")).unwrap(),
        "hello"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn manual_mark_counts_as_fully_read() {
    let dir = std::env::temp_dir().join(format!("wguard_{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let ctx = ctx_in(&dir);
    let sid = ctx.session_id;
    let write = WriteTool;

    assert!(write_raw(&write, &ctx, "m.txt", "x\ny\n").await);
    read_state::mark_fully_read(sid, &dir.join("m.txt"));

    let w = write
        .execute(json!({"path": "m.txt", "content": "ok"}), &ctx)
        .await
        .unwrap();
    assert!(w.success, "marked path writes freely: {:?}", w.error);

    std::fs::remove_dir_all(&dir).ok();
}
