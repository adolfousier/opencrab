//! Tests the archive guard: `FileService.get_or_create_file` only copies
//! EPHEMERAL files (under `~/.opencrabs/tmp/`) into a project's files dir.
//! Persistent files the agent writes/edits in a real working directory (code
//! in a repo, etc.) must be tracked at their real path, never copied — copying
//! them clutters the project dir and points the tracked path at a stale copy.

use crate::db::Database;
use crate::services::{FileService, ProjectService, ServiceContext, SessionService};

#[tokio::test]
async fn non_tmp_file_is_tracked_in_place_not_archived() {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let ctx = ServiceContext::new(db.pool().clone());
    let sessions = SessionService::new(ctx.clone());
    let projects = ProjectService::new(ctx.clone());
    let files = FileService::new(ctx.clone());

    let project = projects
        .create_project("DevProj".to_string(), None)
        .await
        .unwrap();
    let session = sessions
        .create_session(Some("s".to_string()))
        .await
        .unwrap();
    projects
        .assign_session(session.id, project.id)
        .await
        .unwrap();

    // A file OUTSIDE ~/.opencrabs/tmp (a plain working-dir file, like agent-
    // edited code) must be tracked at its original path — not copied into the
    // project files dir.
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("auth_service.dart");
    std::fs::write(&src, b"class AuthService {}").unwrap();

    let tracked = files
        .get_or_create_file(session.id, src.clone(), None)
        .await
        .unwrap();

    assert_eq!(
        tracked.path, src,
        "a persistent working-dir file must be tracked in place, not archived into the project dir"
    );
}
