//! Tests for auto-title post-processing (`clean_auto_title`).
//!
//! The LLM returns a raw title string. These tests verify the cleanup:
//! trim whitespace, strip surrounding quotes, cap at 60 characters.

use crate::brain::agent::service::AgentService;

mod clean_auto_title {
    use super::*;

    #[test]
    fn basic_title_unchanged() {
        assert_eq!(
            AgentService::clean_auto_title("Rust Refactoring"),
            "Rust Refactoring"
        );
    }

    #[test]
    fn trims_whitespace() {
        assert_eq!(
            AgentService::clean_auto_title("  Hashline Debug  "),
            "Hashline Debug"
        );
    }

    #[test]
    fn strips_double_quotes() {
        assert_eq!(
            AgentService::clean_auto_title("\"Auto Title Fix\""),
            "Auto Title Fix"
        );
    }

    #[test]
    fn strips_single_quotes() {
        assert_eq!(
            AgentService::clean_auto_title("'Session Config'"),
            "Session Config"
        );
    }

    #[test]
    fn caps_at_60_chars() {
        let long = "a".repeat(80);
        let result = AgentService::clean_auto_title(&long);
        assert_eq!(result.len(), 60);
    }

    #[test]
    fn exactly_60_chars_unchanged() {
        let exact = "b".repeat(60);
        assert_eq!(AgentService::clean_auto_title(&exact), exact);
    }

    #[test]
    fn empty_string() {
        assert_eq!(AgentService::clean_auto_title(""), "");
    }

    #[test]
    fn only_whitespace() {
        assert_eq!(AgentService::clean_auto_title("   "), "");
    }

    #[test]
    fn only_quotes() {
        assert_eq!(AgentService::clean_auto_title("\"\""), "");
    }

    #[test]
    fn quotes_and_whitespace() {
        assert_eq!(
            AgentService::clean_auto_title("  \"Clean Title\"  "),
            "Clean Title"
        );
    }

    #[test]
    fn trailing_punctuation_kept() {
        // The LLM prompt says no punctuation but we don't enforce it in code
        assert_eq!(AgentService::clean_auto_title("Fix Bug."), "Fix Bug.");
    }
}

mod is_default_channel_title {
    use super::*;

    #[test]
    fn telegram_dm_title() {
        assert!(AgentService::is_default_channel_title(
            "Telegram: DM John (12345) [chat:67890]"
        ));
    }

    #[test]
    fn telegram_group_title() {
        // Telegram groups have no clear default pattern marker, so they're NOT default
        // to prevent auto-title from firing on every message
        assert!(!AgentService::is_default_channel_title(
            "Telegram: My Group [chat:12345]"
        ));
    }

    #[test]
    fn telegram_auto_titled_not_default() {
        // After auto-title runs, the title should NOT be considered default
        assert!(!AgentService::is_default_channel_title(
            "Telegram: Fix Bug Report [chat:67890]"
        ));
    }

    #[test]
    fn discord_title() {
        assert!(AgentService::is_default_channel_title("Discord: #general"));
    }

    #[test]
    fn slack_title() {
        assert!(AgentService::is_default_channel_title("Slack: #random"));
    }

    #[test]
    fn whatsapp_title() {
        // WhatsApp has no clear default pattern marker, so it's NOT default
        // to prevent auto-title from firing on every message
        assert!(!AgentService::is_default_channel_title(
            "WhatsApp: John Doe"
        ));
    }

    #[test]
    fn trello_title() {
        // Trello has no clear default pattern marker, so it's NOT default
        // to prevent auto-title from firing on every message
        assert!(!AgentService::is_default_channel_title("Trello: My Board"));
    }

    #[test]
    fn custom_title_not_default() {
        assert!(!AgentService::is_default_channel_title(
            "Rust Refactoring Session"
        ));
    }

    #[test]
    fn empty_title_not_default() {
        assert!(!AgentService::is_default_channel_title(""));
    }

    #[test]
    fn partial_prefix_not_default() {
        assert!(!AgentService::is_default_channel_title("Telegram"));
        assert!(!AgentService::is_default_channel_title("TelegramBot: test"));
    }

    #[test]
    fn new_chat_is_default() {
        assert!(AgentService::is_default_channel_title("New Chat"));
    }
}

mod extract_channel_prefix {
    use super::*;

    #[test]
    fn telegram_prefix() {
        assert_eq!(
            AgentService::extract_channel_prefix("Telegram: DM John"),
            "Telegram: "
        );
    }

    #[test]
    fn discord_prefix() {
        assert_eq!(
            AgentService::extract_channel_prefix("Discord: #general"),
            "Discord: "
        );
    }

    #[test]
    fn slack_prefix() {
        assert_eq!(
            AgentService::extract_channel_prefix("Slack: #random"),
            "Slack: "
        );
    }

    #[test]
    fn whatsapp_prefix() {
        assert_eq!(
            AgentService::extract_channel_prefix("WhatsApp: John"),
            "WhatsApp: "
        );
    }

    #[test]
    fn trello_prefix() {
        assert_eq!(
            AgentService::extract_channel_prefix("Trello: Board"),
            "Trello: "
        );
    }

    #[test]
    fn no_prefix() {
        assert_eq!(AgentService::extract_channel_prefix("New Chat"), "");
        assert_eq!(AgentService::extract_channel_prefix("Custom Title"), "");
        assert_eq!(AgentService::extract_channel_prefix(""), "");
    }
}
