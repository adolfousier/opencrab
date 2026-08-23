//! Regression tests for #1170: the shared `ChannelFactory` must propagate the
//! sub-agent manager into every agent service it builds. Before the fix only
//! the cron daemon's factory was wired (`ui.rs` daemon path), so `tasks_list`
//! read an empty registry in every chat-channel session
//! (Telegram/WhatsApp/Discord/Slack) while CLI and cron worked fine.

use crate::brain::provider::Provider;
use crate::channels::ChannelFactory;
use crate::config::Config;
use crate::db::Database;
use crate::services::ServiceContext;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, watch};
use uuid::Uuid;

use super::agent_service_mocks::MockProvider;

async fn test_factory() -> ChannelFactory {
    let db = Database::connect_in_memory().await.unwrap();
    let context = ServiceContext::new(db.pool().clone());
    let (_, config_rx) = watch::channel(Config::default());
    ChannelFactory::new(
        Arc::new(MockProvider) as Arc<dyn Provider>,
        context,
        "test brain".to_string(),
        PathBuf::from("/tmp"),
        PathBuf::from("/tmp/oc_test_brain"),
        Arc::new(Mutex::new(None)),
        config_rx,
    )
}

/// The #1170 contract: once wired, EVERY service the factory builds carries
/// the sub-agent manager, so channel sessions get a live sub-agent section.
#[tokio::test]
async fn factory_propagates_subagent_manager_to_built_services() {
    let factory = test_factory().await;
    let manager = Arc::new(crate::brain::tools::subagent::SubAgentManager::new());
    factory.set_subagent_manager(manager);

    let service = factory.create_agent_service_full(None, None).await;
    assert!(
        service.subagent_manager().is_some(),
        "channel-built service must carry the sub-agent manager (#1170)"
    );
}

/// Documents the pre-fix failure mode: an unwired factory yields `None`, so
/// the positive assertion above cannot pass trivially.
#[tokio::test]
async fn factory_without_wiring_yields_none() {
    let factory = test_factory().await;
    let service = factory.create_agent_service_full(None, None).await;
    assert!(service.subagent_manager().is_none());
}
