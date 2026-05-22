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
        assert!(AgentService::is_default_channel_title(
            "Telegram: My Group [chat:12345]"
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
        assert!(AgentService::is_default_channel_title("WhatsApp: John Doe"));
    }

    #[test]
    fn trello_title() {
        assert!(AgentService::is_default_channel_title("Trello: My Board"));
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
}
