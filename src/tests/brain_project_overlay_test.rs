//! Per-project brain overlay integration test for `load_brain_file`.
//!
//! Exercises the path the existing loader unit tests cannot: a session that
//! belongs to a project carrying its own brain file under `projects/<slug>/`.
//! Asserts that the project file loads ON TOP of the profile-level file
//! (append, never replace), is clearly labelled with the project name, and is
//! still surfaced when the profile has no counterpart. A task-local profile
//! home (`with_profile_home_async`) keeps every on-disk file under a throwaway
//! profile so the real `~/.opencrabs/` home is never touched.

use crate::brain::tools::load_brain_file::LoadBrainFileTool;
use crate::brain::tools::{Tool, ToolExecutionContext};
use crate::config::profile::{home_for_profile, with_profile_home_async};
use crate::db::Database;
use crate::db::models::{Project, Session};
use crate::db::repository::{ProjectRepository, SessionRepository};
use crate::services::file::slugify_project_name;
use crate::services::{ProjectService, ServiceContext};
use uuid::Uuid;

// ── helpers ──────────────────────────────────────────────────────────────────

/// In-memory DB with one project and a session assigned to it.
/// Returns the service context, the session id, and the project name.
async fn project_session() -> (ServiceContext, Uuid, String) {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let pool = db.pool().clone();

    let project = Project::new("Overlay Demo".into(), None);
    ProjectRepository::new(pool.clone())
        .create(&project)
        .await
        .unwrap();

    let session = Session::new(Some("chat".into()), Some("m".into()), None);
    SessionRepository::new(pool.clone())
        .create(&session)
        .await
        .unwrap();
    ProjectRepository::new(pool.clone())
        .assign_session(session.id, project.id)
        .await
        .unwrap();

    (ServiceContext::new(pool), session.id, project.name)
}

fn ctx_with(svc: ServiceContext, session_id: Uuid) -> ToolExecutionContext {
    let mut ctx = ToolExecutionContext::new(session_id);
    ctx.service_context = Some(svc);
    ctx
}

/// A throwaway profile name so the on-disk overlay lives under
/// `~/.opencrabs/profiles/<unique>/`, never the real default home.
fn throwaway_profile() -> String {
    format!("test-overlay-{}", Uuid::new_v4())
}

// ── tests ────────────────────────────────────────────────────────────────────

/// Project AGENTS.md appends AFTER the profile AGENTS.md, labelled, with both
/// kept: a project file ADDS context, it never replaces (and so can never
/// silently drop) a profile-level hard rule.
#[tokio::test]
async fn overlay_appends_on_top_of_profile_brain() {
    let (svc, session_id, pname) = project_session().await;
    let profile = throwaway_profile();

    let out = with_profile_home_async(Some(&profile), async {
        let home = crate::config::opencrabs_home();
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(
            home.join("AGENTS.md"),
            "# Profile rule\nNever push without approval.",
        )
        .unwrap();

        let pdir = ProjectService::projects_dir().join(slugify_project_name(&pname));
        std::fs::create_dir_all(&pdir).unwrap();
        std::fs::write(
            pdir.join("AGENTS.md"),
            "# Project rule\nThis repo uses cargo clippy --all-features.",
        )
        .unwrap();

        let ctx = ctx_with(svc.clone(), session_id);
        LoadBrainFileTool
            .execute(serde_json::json!({"name": "AGENTS.md"}), &ctx)
            .await
            .unwrap()
    })
    .await;

    let _ = std::fs::remove_dir_all(home_for_profile(Some(&profile)));

    assert!(out.success);
    let body = out.output;
    assert!(
        body.contains("Never push without approval."),
        "profile content missing:\n{body}"
    );
    assert!(
        body.contains("cargo clippy --all-features"),
        "overlay content missing:\n{body}"
    );
    assert!(
        body.contains(&format!("(project: {pname} overlay)")),
        "overlay must be labelled with the project name:\n{body}"
    );
    let prof_idx = body.find("Never push").unwrap();
    let proj_idx = body.find("cargo clippy").unwrap();
    assert!(
        prof_idx < proj_idx,
        "overlay must append AFTER the profile content, not before:\n{body}"
    );
}

/// When the profile has no counterpart file, the project overlay is still
/// surfaced on its own rather than the loader reporting "not found".
#[tokio::test]
async fn overlay_surfaces_when_profile_file_absent() {
    let (svc, session_id, pname) = project_session().await;
    let profile = throwaway_profile();

    let out = with_profile_home_async(Some(&profile), async {
        // Deliberately write NO profile-level TOOLS.md.
        let pdir = ProjectService::projects_dir().join(slugify_project_name(&pname));
        std::fs::create_dir_all(&pdir).unwrap();
        std::fs::write(
            pdir.join("TOOLS.md"),
            "# Project tools\nUse the local mcp gateway.",
        )
        .unwrap();

        let ctx = ctx_with(svc.clone(), session_id);
        LoadBrainFileTool
            .execute(serde_json::json!({"name": "TOOLS.md"}), &ctx)
            .await
            .unwrap()
    })
    .await;

    let _ = std::fs::remove_dir_all(home_for_profile(Some(&profile)));

    assert!(out.success);
    assert!(
        out.output.contains("Use the local mcp gateway."),
        "overlay-only content missing:\n{}",
        out.output
    );
    assert!(
        out.output.contains(&format!("(project: {pname} overlay)")),
        "overlay label missing:\n{}",
        out.output
    );
}

/// A session with no project must get the plain profile behaviour, no overlay
/// section appended, even with a service_context present.
#[tokio::test]
async fn no_overlay_when_session_has_no_project() {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let pool = db.pool().clone();

    let session = Session::new(Some("chat".into()), Some("m".into()), None);
    SessionRepository::new(pool.clone())
        .create(&session)
        .await
        .unwrap();
    let svc = ServiceContext::new(pool);
    let profile = throwaway_profile();

    let out = with_profile_home_async(Some(&profile), async {
        let ctx = ctx_with(svc.clone(), session.id);
        LoadBrainFileTool
            .execute(serde_json::json!({"name": "AGENTS.md"}), &ctx)
            .await
            .unwrap()
    })
    .await;

    let _ = std::fs::remove_dir_all(home_for_profile(Some(&profile)));

    assert!(out.success);
    assert!(
        !out.output.contains("overlay"),
        "a project-less session must not append any overlay:\n{}",
        out.output
    );
}
