//! Slack Connect Tool
//!
//! Agent-callable tool that connects a Slack bot at runtime via Socket Mode.
//! Accepts bot token + app token, saves to config/keyring, spawns the bot,
//! and waits for a successful connection.

use super::error::Result;
use super::r#trait::{Tool, ToolCapability, ToolExecutionContext, ToolResult};
use crate::channels::ChannelFactory;
use crate::slack::SlackState;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;

/// Tool that connects a Slack bot by accepting bot + app tokens.
pub struct SlackConnectTool {
    channel_factory: Arc<ChannelFactory>,
    slack_state: Arc<SlackState>,
}

impl SlackConnectTool {
    pub fn new(
        channel_factory: Arc<ChannelFactory>,
        slack_state: Arc<SlackState>,
    ) -> Self {
        Self {
            channel_factory,
            slack_state,
        }
    }
}

#[async_trait]
impl Tool for SlackConnectTool {
    fn name(&self) -> &str {
        "slack_connect"
    }

    fn description(&self) -> &str {
        "Connect a Slack bot to OpenCrabs via Socket Mode. Requires two tokens: \
         a Bot Token (xoxb-...) and an App-Level Token (xapp-...). \
         The user must create an app at https://api.slack.com/apps, enable Socket Mode, \
         add an App-Level Token with 'connections:write' scope, and install the app to their workspace. \
         Call this when the user asks to connect or set up Slack."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "bot_token": {
                    "type": "string",
                    "description": "Slack Bot Token (starts with xoxb-)"
                },
                "app_token": {
                    "type": "string",
                    "description": "Slack App-Level Token (starts with xapp-). Required for Socket Mode."
                },
                "allowed_ids": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Slack user IDs to allow (e.g. 'U12345678'). If empty, all messages accepted."
                }
            },
            "required": ["bot_token", "app_token"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::Network, ToolCapability::SystemModification]
    }

    async fn execute(&self, input: Value, _context: &ToolExecutionContext) -> Result<ToolResult> {
        // Check if already connected
        if self.slack_state.is_connected().await {
            return Ok(ToolResult::success(
                "Slack is already connected.".to_string(),
            ));
        }

        let bot_token = match input.get("bot_token").and_then(|v| v.as_str()) {
            Some(t) if !t.is_empty() => t.to_string(),
            _ => {
                return Ok(ToolResult::error(
                    "Missing or empty 'bot_token' parameter. \
                     The user needs their Slack Bot Token (starts with xoxb-)."
                        .to_string(),
                ));
            }
        };

        let app_token = match input.get("app_token").and_then(|v| v.as_str()) {
            Some(t) if !t.is_empty() => t.to_string(),
            _ => {
                return Ok(ToolResult::error(
                    "Missing or empty 'app_token' parameter. \
                     The user needs their Slack App-Level Token (starts with xapp-). \
                     This is required for Socket Mode."
                        .to_string(),
                ));
            }
        };

        let allowed_ids: Vec<String> = input
            .get("allowed_ids")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        // Save tokens to keyring for persistence
        if let Ok(entry) = keyring::Entry::new("opencrabs", "slack_bot_token") {
            let _ = entry.set_password(&bot_token);
        }
        if let Ok(entry) = keyring::Entry::new("opencrabs", "slack_app_token") {
            let _ = entry.set_password(&app_token);
        }

        // Persist enabled state to config
        let _ = crate::config::Config::write_key("channels.slack", "enabled", "true");

        // Create and spawn the Slack agent
        let factory = self.channel_factory.clone();
        let agent = factory.create_agent_service();
        let service_context = factory.service_context();
        let shared_session = factory.shared_session_id();
        let slack_state = self.slack_state.clone();

        let sl_agent = crate::slack::SlackAgent::new(
            agent,
            service_context,
            allowed_ids,
            shared_session,
            slack_state.clone(),
        );

        let _handle = sl_agent.start(bot_token, app_token);

        // Wait for the bot to connect (SlackAgent sets slack_state on connect)
        let timeout = Duration::from_secs(30);
        let start = std::time::Instant::now();
        loop {
            if slack_state.is_connected().await {
                return Ok(ToolResult::success(
                    "Slack bot connected successfully via Socket Mode! \
                     Now listening for messages. Connection persists across restarts."
                        .to_string(),
                ));
            }
            if start.elapsed() > timeout {
                return Ok(ToolResult::error(
                    "Timed out waiting for Slack bot to connect (30s). \
                     Check that both tokens are valid. The Bot Token should start with 'xoxb-' \
                     and the App Token with 'xapp-'. Socket Mode must be enabled in your Slack app settings."
                        .to_string(),
                ));
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
}
