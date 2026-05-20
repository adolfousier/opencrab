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
