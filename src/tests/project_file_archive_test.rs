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
use uuid::Uuid;

/// HOME-override harness so `project_files_dir` and the `~/.opencrabs/tmp/`
/// ephemeral check resolve under a temp home instead of the real `~/.opencrabs/`.
struct HomeGuard {
    prev_home: Option<std::ffi::OsString>,
    prev_userprofile: Option<std::ffi::OsString>,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl HomeGuard {
    fn new(temp: &std::path::Path) -> Self {
        let lock = crate::tests::HOME_ENV_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let prev_home = std::env::var_os("HOME");
        let prev_userprofile = std::env::var_os("USERPROFILE");
        // SAFETY: HOME_ENV_LOCK serializes HOME mutation across the suite.
        unsafe {
            std::env::set_var("HOME", temp);
            std::env::set_var("USERPROFILE", temp);
        }
        Self {
            prev_home,
            prev_userprofile,
            _lock: lock,
        }
    }
}

impl Drop for HomeGuard {
    fn drop(&mut self) {
        match self.prev_home.take() {
            Some(v) => unsafe { std::env::set_var("HOME", v) },
            None => unsafe { std::env::remove_var("HOME") },
        }
        match self.prev_userprofile.take() {
            Some(v) => unsafe { std::env::set_var("USERPROFILE", v) },
            None => unsafe { std::env::remove_var("USERPROFILE") },
        }
    }
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
    let temp = tempfile::tempdir().unwrap();
    let _home = HomeGuard::new(temp.path());

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
}

#[cfg(unix)]
#[tokio::test]
async fn persistent_local_file_is_symlinked_into_project() {
    let temp = tempfile::tempdir().unwrap();
    let _home = HomeGuard::new(temp.path());

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
}
