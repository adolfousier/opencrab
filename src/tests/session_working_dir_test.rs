//! Session Working Directory Tests
//!
//! Tests for persisting and restoring per-session working directories,
//! the update checker's semver comparison, and source build detection.

// --- Session DB persistence ---

mod session_db {
    use crate::db::Database;
    use crate::db::models::Session;
    use crate::services::{ServiceContext, SessionService};

    async fn setup() -> SessionService {
        let db = Database::connect_in_memory().await.unwrap();
        db.run_migrations().await.unwrap();
        SessionService::new(ServiceContext::new(db.pool().clone()))
    }

    #[tokio::test]
    async fn new_session_has_no_working_directory() {
        let svc = setup().await;
        let session = svc.create_session(Some("Test".into())).await.unwrap();
        assert!(session.working_directory.is_none());
    }

    #[tokio::test]
    async fn update_working_directory_persists() {
        let svc = setup().await;
        let session = svc.create_session(Some("Test".into())).await.unwrap();

        svc.update_session_working_directory(session.id, Some("/tmp/project".into()))
            .await
            .unwrap();

        let loaded = svc.get_session_required(session.id).await.unwrap();
        assert_eq!(loaded.working_directory, Some("/tmp/project".into()));
    }

    #[tokio::test]
    async fn update_working_directory_to_none() {
        let svc = setup().await;
        let session = svc.create_session(Some("Test".into())).await.unwrap();

        // Set it
        svc.update_session_working_directory(session.id, Some("/tmp/a".into()))
            .await
            .unwrap();

        // Clear it
        svc.update_session_working_directory(session.id, None)
            .await
            .unwrap();

        let loaded = svc.get_session_required(session.id).await.unwrap();
        assert!(loaded.working_directory.is_none());
    }

    #[tokio::test]
    async fn working_directory_survives_session_update() {
        let svc = setup().await;
        let mut session = svc.create_session(Some("Test".into())).await.unwrap();

        // Set working dir
        svc.update_session_working_directory(session.id, Some("/home/user/proj".into()))
            .await
            .unwrap();

        // Update title (full session update should preserve working_directory)
        session = svc.get_session_required(session.id).await.unwrap();
        session.title = Some("Renamed".into());
        svc.update_session(&session).await.unwrap();

        let loaded = svc.get_session_required(session.id).await.unwrap();
        assert_eq!(loaded.title, Some("Renamed".into()));
        assert_eq!(loaded.working_directory, Some("/home/user/proj".into()));
    }

    #[tokio::test]
    async fn working_directory_included_in_list() {
        let svc = setup().await;
        let session = svc.create_session(Some("Listed".into())).await.unwrap();

        svc.update_session_working_directory(session.id, Some("/srv/app".into()))
            .await
            .unwrap();

        let options = crate::db::repository::SessionListOptions {
            include_archived: false,
            limit: None,
            offset: 0,
            query: None,
        };
        let sessions = svc.list_sessions(options).await.unwrap();
        let found = sessions.iter().find(|s| s.id == session.id).unwrap();
        assert_eq!(found.working_directory, Some("/srv/app".into()));
    }

    #[tokio::test]
    async fn working_directory_in_new_session_via_create() {
        let session = Session::new(
            Some("Manual".into()),
            Some("model".into()),
            Some("provider".into()),
        );
        assert!(session.working_directory.is_none());
    }

    #[tokio::test]
    async fn multiple_sessions_different_directories() {
        let svc = setup().await;
        let s1 = svc.create_session(Some("Project A".into())).await.unwrap();
        let s2 = svc.create_session(Some("Project B".into())).await.unwrap();
        let s3 = svc.create_session(Some("No dir".into())).await.unwrap();

        svc.update_session_working_directory(s1.id, Some("/home/user/project-a".into()))
            .await
            .unwrap();
        svc.update_session_working_directory(s2.id, Some("/home/user/project-b".into()))
            .await
            .unwrap();

        let loaded1 = svc.get_session_required(s1.id).await.unwrap();
        let loaded2 = svc.get_session_required(s2.id).await.unwrap();
        let loaded3 = svc.get_session_required(s3.id).await.unwrap();

        assert_eq!(
            loaded1.working_directory,
            Some("/home/user/project-a".into())
        );
        assert_eq!(
            loaded2.working_directory,
            Some("/home/user/project-b".into())
        );
        assert!(loaded3.working_directory.is_none());
    }
}

// --- create_channel_session working_directory inheritance (#258, #263) ---

mod channel_session_wd_inherit {
    use crate::channels::session_init::create_channel_session;
    use crate::db::Database;
    use crate::services::{ServiceContext, SessionService};

    async fn setup() -> SessionService {
        let db = Database::connect_in_memory().await.unwrap();
        db.run_migrations().await.unwrap();
        SessionService::new(ServiceContext::new(db.pool().clone()))
    }

    #[tokio::test]
    async fn inherits_working_directory_from_given_session() {
        let svc = setup().await;

        // The session that received /new (same chat) has a working directory.
        let prior = svc.create_session(Some("Prior".into())).await.unwrap();
        svc.update_session_working_directory(prior.id, Some("/home/user/my-project".into()))
            .await
            .unwrap();
        let prior = svc.get_session_required(prior.id).await.unwrap();

        // create_channel_session inherits the working directory from it.
        let new_session = create_channel_session(&svc, Some("Channel".into()), Some(&prior))
            .await
            .unwrap();

        assert_eq!(
            new_session.working_directory,
            Some("/home/user/my-project".into()),
            "create_channel_session must inherit working_directory from the session that received /new"
        );
    }

    #[tokio::test]
    async fn no_working_directory_when_no_prior_session() {
        let svc = setup().await;

        // Brand-new chat: no same-chat session to inherit from.
        let new_session = create_channel_session(&svc, Some("First".into()), None)
            .await
            .unwrap();

        assert!(
            new_session.working_directory.is_none(),
            "create_channel_session should set working_directory=None when no prior session is given"
        );
    }

    #[tokio::test]
    async fn no_working_directory_when_prior_has_none() {
        let svc = setup().await;

        // The same-chat session exists but has no working directory.
        let prior = svc
            .create_session(Some("Prior No WD".into()))
            .await
            .unwrap();

        let new_session = create_channel_session(&svc, Some("Channel".into()), Some(&prior))
            .await
            .unwrap();

        assert!(
            new_session.working_directory.is_none(),
            "create_channel_session should be None when the given session has no working_directory"
        );
    }

    #[tokio::test]
    async fn uses_given_session_wd_not_global_latest() {
        let svc = setup().await;

        // Session A is the same-chat session that received /new; it has /project-a.
        let s_a = svc.create_session(Some("A".into())).await.unwrap();
        svc.update_session_working_directory(s_a.id, Some("/project-a".into()))
            .await
            .unwrap();
        let s_a = svc.get_session_required(s_a.id).await.unwrap();

        // Session B is created later (globally most recent) with /project-b and
        // belongs to a different chat. Its cwd must NOT leak into the new
        // session (#263 — the cross-channel working-directory bleed).
        let s_b = svc.create_session(Some("B".into())).await.unwrap();
        svc.update_session_working_directory(s_b.id, Some("/project-b".into()))
            .await
            .unwrap();

        let new_session = create_channel_session(&svc, Some("Channel".into()), Some(&s_a))
            .await
            .unwrap();

        assert_eq!(
            new_session.working_directory,
            Some("/project-a".into()),
            "working_directory must come from the given same-chat session (A), not the global most-recent (B)"
        );
    }
}

// --- Update checker semver comparison ---

mod update_checker {
    use crate::brain::tools::evolve::is_newer;

    #[test]
    fn newer_patch() {
        assert!(is_newer("0.2.58", "0.2.57"));
    }

    #[test]
    fn same_version() {
        assert!(!is_newer("0.2.57", "0.2.57"));
    }

    #[test]
    fn older_version_not_newer() {
        assert!(!is_newer("0.2.57", "0.2.58"));
    }

    #[test]
    fn newer_minor() {
        assert!(is_newer("0.3.0", "0.2.99"));
    }

    #[test]
    fn newer_major() {
        assert!(is_newer("1.0.0", "0.99.99"));
    }

    #[test]
    fn older_major_not_newer() {
        assert!(!is_newer("0.9.0", "1.0.0"));
    }

    #[test]
    fn equal_major_older_minor() {
        assert!(!is_newer("1.0.5", "1.1.0"));
    }

    #[test]
    fn two_segment_versions() {
        assert!(is_newer("1.1", "1.0"));
        assert!(!is_newer("1.0", "1.1"));
    }
}
