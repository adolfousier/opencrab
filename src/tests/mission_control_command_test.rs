//! `/mission-control` is a built-in channel command: it must route through the
//! text-command path (so channels send its body) and show up in `/help`.

use crate::channels::commands::{ChannelCommand, format_help, try_execute_text_command};

#[tokio::test]
async fn mission_control_command_returns_its_body() {
    let cmd = ChannelCommand::MissionControl("the mission control report".to_string());
    assert_eq!(
        try_execute_text_command(&cmd).await,
        Some("the mission control report".to_string())
    );
}

#[test]
fn help_lists_the_mission_control_command() {
    assert!(format_help().contains("/mission-control"));
}
