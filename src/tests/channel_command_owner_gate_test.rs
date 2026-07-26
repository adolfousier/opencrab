//! Security: `/cd` browses the host filesystem, so on channels it must be
//! restricted to the bot owner. A non-owner on the allowlist must not be able
//! to navigate the owner's files.

use crate::channels::commands::{ChannelCommand, handle_command};
use crate::tests::agent_service_mocks::create_test_service_full;

#[tokio::test]
async fn cd_is_denied_for_non_owner() {
    let (agent, svc, sid) = create_test_service_full().await;
    let cmd = handle_command("/cd", sid, &agent, &svc, false, None).await;
    match cmd {
        ChannelCommand::UnknownCommand(msg) => {
            assert!(
                msg.to_lowercase().contains("owner"),
                "denial should mention owner-only, got: {msg}"
            );
        }
        _ => panic!("non-owner /cd must be denied with an owner-only UnknownCommand"),
    }
}

#[tokio::test]
async fn cd_is_allowed_for_owner() {
    let (agent, svc, sid) = create_test_service_full().await;
    let cmd = handle_command("/cd", sid, &agent, &svc, true, None).await;
    assert!(
        matches!(cmd, ChannelCommand::ChangeDir(_)),
        "owner /cd must open the directory browser"
    );
}

#[tokio::test]
async fn cd_with_path_is_also_denied_for_non_owner() {
    let (agent, svc, sid) = create_test_service_full().await;
    let cmd = handle_command("/cd /etc", sid, &agent, &svc, false, None).await;
    assert!(
        matches!(cmd, ChannelCommand::UnknownCommand(_)),
        "non-owner /cd <path> must be denied too"
    );
}

// ── /new is owner-gated like every sibling (#782) ───────────────────────────
// It was the one command without the check, so anyone in an allowlisted group
// could reset the owner's session out from under them.

#[tokio::test]
async fn new_is_denied_for_non_owner() {
    let (agent, svc, sid) = create_test_service_full().await;
    let cmd = handle_command("/new", sid, &agent, &svc, false, None).await;
    match cmd {
        ChannelCommand::UnknownCommand(msg) => assert!(
            msg.to_lowercase().contains("owner"),
            "denial should mention owner-only, got: {msg}"
        ),
        _ => panic!("non-owner /new must be denied — it resets the owner's session"),
    }
}

#[tokio::test]
async fn new_is_allowed_for_owner() {
    let (agent, svc, sid) = create_test_service_full().await;
    let cmd = handle_command("/new", sid, &agent, &svc, true, None).await;
    assert!(matches!(cmd, ChannelCommand::NewSession));
}
