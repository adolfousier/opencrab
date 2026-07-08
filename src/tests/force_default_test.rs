//! Tests for the `force_default` reload push (#466): flag off preserves
//! stored pairs, flag on rewrites non-archived rows to exactly the active
//! section's default pair, archived rows untouched.

use crate::config::{Config, ProviderConfig, ProviderConfigs};
use crate::db::Database;
use crate::services::force_default::{apply_force_default, force_default_pair};
use crate::services::{ServiceContext, SessionService};

fn config_with_minimax(force: bool) -> Config {
    Config {
        providers: ProviderConfigs {
            minimax: Some(ProviderConfig {
                enabled: true,
                api_key: Some("key".into()),
                default_model: Some("MiniMax-M3".into()),
                force_default: force,
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    }
}

#[test]
fn pair_is_none_without_the_flag() {
    assert!(force_default_pair(&config_with_minimax(false)).is_none());
}

#[test]
fn pair_is_the_active_sections_default_with_the_flag() {
    let (provider, model) = force_default_pair(&config_with_minimax(true)).expect("opted in");
    assert_eq!(provider, "minimax");
    assert_eq!(model, "MiniMax-M3");
}

#[tokio::test(flavor = "multi_thread")]
async fn push_rewrites_live_rows_and_skips_archived() {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let svc = SessionService::new(ServiceContext::new(db.pool().clone()));

    let live = svc
        .create_session_with_provider(
            Some("live".into()),
            Some("openrouter".into()),
            Some("old-model".into()),
            None,
        )
        .await
        .unwrap();
    let archived = svc
        .create_session_with_provider(
            Some("archived".into()),
            Some("openrouter".into()),
            Some("old-model".into()),
            None,
        )
        .await
        .unwrap();
    svc.archive_session(archived.id).await.unwrap();

    // Flag off: nothing changes.
    let n = apply_force_default(&config_with_minimax(false), &svc)
        .await
        .unwrap();
    assert_eq!(n, 0);

    // Flag on: the live row switches, the archived row is untouched.
    let n = apply_force_default(&config_with_minimax(true), &svc)
        .await
        .unwrap();
    assert_eq!(n, 1);

    let live_after = svc.get_session(live.id).await.unwrap().unwrap();
    assert_eq!(live_after.provider_name.as_deref(), Some("minimax"));
    assert_eq!(live_after.model.as_deref(), Some("MiniMax-M3"));

    let archived_after = svc.get_session(archived.id).await.unwrap().unwrap();
    assert_eq!(archived_after.provider_name.as_deref(), Some("openrouter"));
    assert_eq!(archived_after.model.as_deref(), Some("old-model"));

    // Idempotent: a second reload touches nothing.
    let n = apply_force_default(&config_with_minimax(true), &svc)
        .await
        .unwrap();
    assert_eq!(n, 0);
}
