//! Tests `FileService.get_or_create_file`'s project archiving:
//! - **Ephemeral shares** (under `~/.opencrabs/tmp/` — channel/clipboard/web
//!   downloads) are **copied** into the project files dir (the source is cleaned
//!   up, so a copy is required to keep the artifact).
//! - **Persistent local files** (drag-drop, agent-produced) are **symlinked**
//!   into the project files dir — no duplication, stays in sync with the original.
//! - **Repository code** (inside a git repo) is tracked at its real path, never
//!   archived.

use crate::db::Database;
use crate::services::{FileService, ProjectService, ServiceContext, SessionService};
use crate::config::profile::{home_for_profile, with_profile_home_async};
use uuid::Uuid;

fn setup_profile_home(home: &std::path::Path) {
    std::fs::create_dir_all(home).expect("create profile home");
}

async fn project_session(ctx: &ServiceContext) -> Uuid {
    let sessions = SessionService::new(ctx.clone());
    let projects = ProjectService::new(ctx.clone());
    let project = projects
        .create_project("Proj".to_string(), None)
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
    session.id
}

#[tokio::test]
async fn repo_code_is_tracked_in_place_not_archived() {
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

    // Agent-edited code inside a git repo must be tracked at its original path,
    // never copied into the project files dir — it lives in (and changes on)
    // the repo. A `.git` dir marks the repo.
    let repo = tempfile::tempdir().unwrap();
    std::fs::create_dir(repo.path().join(".git")).unwrap();
    let src = repo.path().join("lib").join("auth_service.dart");
    std::fs::create_dir_all(src.parent().unwrap()).unwrap();
    std::fs::write(&src, b"class AuthService {}").unwrap();

    let tracked = files
        .get_or_create_file(session.id, src.clone(), None)
        .await
        .unwrap();

    assert_eq!(
        tracked.path, src,
        "repository code must be tracked in place, not archived into the project dir"
    );
}

#[tokio::test]
async fn ephemeral_share_is_copied_into_project() {
    let profile = format!("test_archive_ephemeral_{}", uuid::Uuid::new_v4());
    let home = home_for_profile(Some(&profile));
    setup_profile_home(&home);

    with_profile_home_async(Some(&profile), async {
        let db = Database::connect_in_memory().await.unwrap();
        db.run_migrations().await.unwrap();
        let ctx = ServiceContext::new(db.pool().clone());
        let files = FileService::new(ctx.clone());
        let sid = project_session(&ctx).await;

        // A channel / clipboard / web download lands under ~/.opencrabs/tmp/.
        let tmp_dir = crate::config::opencrabs_home().join("tmp");
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let src = tmp_dir.join("photo.png");
        std::fs::write(&src, b"PNGDATA").unwrap();

        let tracked = files
            .get_or_create_file(sid, src.clone(), None)
            .await
            .unwrap();

        let projects_root = crate::config::opencrabs_home().join("projects");
        assert!(
            tracked.path.starts_with(&projects_root),
            "ephemeral share must be archived into the project dir: {:?}",
            tracked.path
        );
        assert!(
            !std::fs::symlink_metadata(&tracked.path)
                .unwrap()
                .file_type()
                .is_symlink(),
            "ephemeral share must be COPIED, not symlinked"
        );
        assert_eq!(std::fs::read(&tracked.path).unwrap(), b"PNGDATA");
    })
    .await;
}

#[cfg(unix)]
#[tokio::test]
async fn persistent_local_file_is_symlinked_into_project() {
    let profile = format!("test_archive_symlink_{}", uuid::Uuid::new_v4());
    let home = home_for_profile(Some(&profile));
    setup_profile_home(&home);

    with_profile_home_async(Some(&profile), async {
        let db = Database::connect_in_memory().await.unwrap();
        db.run_migrations().await.unwrap();
        let ctx = ServiceContext::new(db.pool().clone());
        let files = FileService::new(ctx.clone());
        let sid = project_session(&ctx).await;

        // A persistent local file the user shared — NOT under ~/.opencrabs/tmp/ and
        // not inside a git repo.
        let local = crate::config::opencrabs_home().join("shared");
        std::fs::create_dir_all(&local).unwrap();
        let src = local.join("doc.pdf");
        std::fs::write(&src, b"PDFDATA").unwrap();

        let tracked = files
            .get_or_create_file(sid, src.clone(), None)
            .await
            .unwrap();

        let projects_root = crate::config::opencrabs_home().join("projects");
        assert!(
            tracked.path.starts_with(&projects_root),
            "local file must be archived into the project dir: {:?}",
            tracked.path
        );
        assert!(
            std::fs::symlink_metadata(&tracked.path)
                .unwrap()
                .file_type()
                .is_symlink(),
            "persistent local file must be SYMLINKED into the project, not copied"
        );
        // The symlink resolves to the original content.
        assert_eq!(std::fs::read(&tracked.path).unwrap(), b"PDFDATA");
    })
    .await;
}
