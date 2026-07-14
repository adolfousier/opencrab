//! Regression: a channel command must still resolve when the handler has
//! appended an auto-injected media marker to the text (#570).
//!
//! In a group, when an incoming message has no photo of its own, the Telegram
//! handler staples any photo shared in the last 300s onto the text as an
//! `<<IMG:path>>` marker (so a follow-up can reference a just-shared image
//! without re-attaching it). That marker landed BEFORE command detection, so a
//! clean `/models` arrived as `"/models\n<<IMG:...>>"`, matched neither the
//! exact form nor the `"/models "` prefix, and fell through to the agent as a
//! prose prompt. `handle_command` now strips `<<IMG:...>>` / `<<VID:...>>`
//! markers before matching, so commands are immune regardless of how the marker
//! was appended.

use crate::channels::commands::{ChannelCommand, handle_command};
use crate::tests::agent_service_mocks::create_test_service_full;

#[tokio::test]
async fn models_resolves_with_trailing_image_marker() {
    let (agent, svc, sid) = create_test_service_full().await;
    let text = "/models\n<<IMG:/tmp/photo--100-123.jpg>>";
    let cmd = handle_command(text, sid, &agent, &svc, true, None).await;
    assert!(
        matches!(cmd, ChannelCommand::Models(_)),
        "/models with an appended <<IMG:>> marker must still open the picker, got {:?}",
        std::mem::discriminant(&cmd)
    );
}

#[tokio::test]
async fn models_resolves_with_botname_and_image_marker() {
    // The realistic shape: Telegram's `@botname` suffix AND the auto-injected
    // photo marker together.
    let (agent, svc, sid) = create_test_service_full().await;
    let text = "/models@opencrabsbot\n<<IMG:/tmp/photo--100-123.jpg>>";
    let cmd = handle_command(text, sid, &agent, &svc, true, None).await;
    assert!(
        matches!(cmd, ChannelCommand::Models(_)),
        "/models@bot with an appended <<IMG:>> marker must still open the picker, got {:?}",
        std::mem::discriminant(&cmd)
    );
}

#[tokio::test]
async fn command_with_argument_and_image_marker_keeps_argument() {
    // A media marker must not swallow a real command argument.
    let (agent, svc, sid) = create_test_service_full().await;
    let text = "/model groq/llama-3.3-70b\n<<IMG:/tmp/photo--100-123.jpg>>";
    let cmd = handle_command(text, sid, &agent, &svc, true, None).await;
    assert!(
        matches!(cmd, ChannelCommand::ModelSwitched(_)),
        "/model <arg> with an appended marker must switch the model, got {:?}",
        std::mem::discriminant(&cmd)
    );
}
