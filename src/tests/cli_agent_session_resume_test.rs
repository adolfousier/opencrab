//! #1368: `agent --session <prefix|uuid>` resumes a session instead of
//! silently discarding the flag, and every output format echoes the id so
//! scripts can close the loop without scraping `session list`.
//!
//! These tests pin the resolve-or-create seam shared by `cmd_run` and
//! `cmd_agent_interactive` (`resolve_or_create_session`). The pure prefix
//! rules (case-insensitivity, ambiguity candidates) are already pinned by
//! `cli_session_id_prefix_test` (#1340/#1366) and are not restated here.

use crate::cli::session_resolve::resolve_or_create_session;
use crate::db::Database;
use crate::services::{ServiceContext, SessionService};

async fn service() -> SessionService {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    SessionService::new(ServiceContext::new(db.pool().clone()))
}

#[tokio::test]
async fn none_creates_fresh_session_with_default_title() {
    let svc = service().await;
    let session = resolve_or_create_session(&svc, None, "CLI Run")
        .await
        .unwrap();
    assert_eq!(session.title.as_deref(), Some("CLI Run"));
}

#[tokio::test]
async fn eight_char_prefix_resumes_the_matching_session() {
    let svc = service().await;
    let target = svc
        .create_session(Some("orchestrator".to_string()))
        .await
        .unwrap();
    let _other = svc
        .create_session(Some("worker".to_string()))
        .await
        .unwrap();
    // Exactly what a user copies from `session list` or the json output.
    let prefix = &target.id.to_string()[..8];
    let resumed = resolve_or_create_session(&svc, Some(prefix), "CLI Run")
        .await
        .unwrap();
    assert_eq!(resumed.id, target.id, "prefix must resume, not create");
}

#[tokio::test]
async fn unknown_prefix_errors_without_guessing() {
    let svc = service().await;
    let err = resolve_or_create_session(&svc, Some("deadbeef"), "CLI Run")
        .await
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("no session id starts with 'deadbeef'"),
        "got: {err}"
    );
}

#[tokio::test]
async fn full_uuid_absent_from_db_is_reported_not_found() {
    // Fast-path parity with `session get` (#1340): a well-formed UUID the
    // DB does not know surfaces "session not found" rather than a prefix
    // error — the caller learns the truth about the id they supplied.
    let svc = service().await;
    let stranger = uuid::Uuid::new_v4();
    let err = resolve_or_create_session(&svc, Some(&stranger.to_string()), "CLI Run")
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("session not found"), "got: {err}");
}
