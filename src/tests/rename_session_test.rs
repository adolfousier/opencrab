//! Tests for the `rename_session` tool: success, clear, missing service
//! context, oversized title.

use crate::brain::tools::rename_session::RenameSessionTool;
use crate::brain::tools::{Tool, ToolExecutionContext};
use crate::db::Database;
use crate::services::{ServiceContext, SessionService};
use serde_json::json;

async fn setup() -> (Database, ServiceContext, uuid::Uuid) {
    let db = Database::connect_in_memory()
        .await
        .expect("connect_in_memory");
    db.run_migrations().await.expect("migrations");
    let svc_ctx = ServiceContext::new(db.pool().clone());
    let session = SessionService::new(svc_ctx.clone())
        .create_session(Some("Initial title".to_string()))
        .await
        .expect("create_session");
    (db, svc_ctx, session.id)
}

#[tokio::test]
async fn rename_session_updates_title() {
    let (_db, svc_ctx, sid) = setup().await;
    let mut ctx = ToolExecutionContext::new(sid);
    ctx.service_context = Some(svc_ctx.clone());

    let result = RenameSessionTool
        .execute(json!({ "title": "Property matching audit" }), &ctx)
        .await
        .expect("execute");

    assert!(result.success, "rename should succeed: {:?}", result.error);
    assert!(
        result.output.contains("Property matching audit"),
        "output should echo the new title, got: {}",
        result.output
    );

    let session = SessionService::new(svc_ctx)
        .get_session(sid)
        .await
        .expect("get_session")
        .expect("session exists");
    assert_eq!(session.title.as_deref(), Some("Property matching audit"));
}

#[tokio::test]
async fn rename_session_empty_string_clears_title() {
    let (_db, svc_ctx, sid) = setup().await;
    let mut ctx = ToolExecutionContext::new(sid);
    ctx.service_context = Some(svc_ctx.clone());

    let result = RenameSessionTool
        .execute(json!({ "title": "   " }), &ctx)
        .await
        .expect("execute");

    assert!(result.success, "clear should succeed: {:?}", result.error);
    assert!(
        result.output.contains("cleared"),
        "output should mention clearing, got: {}",
        result.output
    );

    let session = SessionService::new(svc_ctx)
        .get_session(sid)
        .await
        .expect("get_session")
        .expect("session exists");
    assert_eq!(session.title, None, "title must be None after clear");
}

#[tokio::test]
async fn rename_session_trims_whitespace() {
    let (_db, svc_ctx, sid) = setup().await;
    let mut ctx = ToolExecutionContext::new(sid);
    ctx.service_context = Some(svc_ctx.clone());

    RenameSessionTool
        .execute(json!({ "title": "  spaced out  " }), &ctx)
        .await
        .expect("execute");

    let session = SessionService::new(svc_ctx)
        .get_session(sid)
        .await
        .expect("get_session")
        .expect("session exists");
    assert_eq!(session.title.as_deref(), Some("spaced out"));
}

#[tokio::test]
async fn rename_session_rejects_oversized_title() {
    let (_db, svc_ctx, sid) = setup().await;
    let mut ctx = ToolExecutionContext::new(sid);
    ctx.service_context = Some(svc_ctx.clone());

    let huge = "a".repeat(250);
    let result = RenameSessionTool
        .execute(json!({ "title": huge }), &ctx)
        .await
        .expect("execute");

    assert!(!result.success, "oversized title must be rejected");
    let err = result.error.unwrap_or_default();
    assert!(
        err.contains("too long"),
        "error should mention the size limit, got: {err}"
    );

    let session = SessionService::new(svc_ctx)
        .get_session(sid)
        .await
        .expect("get_session")
        .expect("session exists");
    assert_eq!(
        session.title.as_deref(),
        Some("Initial title"),
        "original title must survive rejection"
    );
}

#[tokio::test]
async fn rename_session_errors_when_service_context_missing() {
    let sid = uuid::Uuid::new_v4();
    let ctx = ToolExecutionContext::new(sid);
    // No service_context set.

    let result = RenameSessionTool
        .execute(json!({ "title": "won't work" }), &ctx)
        .await
        .expect("execute");

    assert!(!result.success);
    let err = result.error.unwrap_or_default();
    assert!(
        err.to_lowercase().contains("service context"),
        "error must mention missing service context, got: {err}"
    );
}

#[test]
fn rename_session_metadata() {
    let tool = RenameSessionTool;
    assert_eq!(tool.name(), "rename_session");
    assert!(
        !tool.requires_approval(),
        "metadata-only update should not require approval"
    );
    assert!(
        tool.capabilities().is_empty(),
        "rename_session has no filesystem/shell/network capabilities"
    );
    let schema = tool.input_schema();
    let props = schema
        .get("properties")
        .and_then(|p| p.as_object())
        .expect("schema has properties");
    assert!(props.contains_key("title"), "schema must declare 'title'");
}
