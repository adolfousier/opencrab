//! Tests for the incident-log accumulation fix (issue #197).
//!
//! The RSI cycle was appending timestamped incident entries to brain files
//! indefinitely. Each entry had a unique date/session, so existing dedup
//! guards let them through as "new content". Three fixes work together:
//!
//! 1. `is_incident_line()` detects ADDED/REPEAT + session entries
//! 2. `strip_incident_lines()` removes them, keeping only the rule
//! 3. `paragraph_exists()` uses stripped content for 4th overlap strategy
//! 4. `looks_like_failure_log()` catches plain-text ADDED/REPEAT entries

mod paragraph_exists_stripped {
    use crate::brain::tools::brain_file_safety::{AppendDedup, filter_duplicate_append};

    #[test]
    fn same_rule_different_date_caught_as_duplicate() {
        let existing = "\
- **Always quote SSH commands**: When passing commands to ssh_exec, wrap the
  inner command in double quotes so special characters (pipes, redirects, etc.)
  survive shell expansion. ADDED 2026-06-11 (session ba623fd1): confirmed via
  feedback_analyze query. Use single quotes for the outer layer if nesting.";

        let new_entry = "\
- **Always quote SSH commands**: When passing commands to ssh_exec, wrap the
  inner command in double quotes so special characters (pipes, redirects, etc.)
  survive shell expansion. ADDED 2026-06-14 (session deadbeef): confirmed via
  feedback_analyze query. Use single quotes for the outer layer if nesting.";

        let result = filter_duplicate_append(existing, new_entry);
        assert!(
            matches!(result, AppendDedup::AllDuplicate),
            "Same rule with different date/session should be caught as duplicate, got: {:?}",
            result,
        );
    }

    #[test]
    fn third_incident_entry_filtered() {
        let existing = "\
- **Quote SSH commands**: Use double quotes.
  ADDED 2026-06-11 (session aaa): confirmed.
  ADDED 2026-06-12 (session bbb): confirmed.";

        let new_entry = "\
- **Quote SSH commands**: Use double quotes.
  ADDED 2026-06-14 (session ccc): confirmed.";

        let result = filter_duplicate_append(existing, new_entry);
        assert!(
            matches!(result, AppendDedup::AllDuplicate),
            "3rd incident of same rule should be blocked, got: {:?}",
            result,
        );
    }

    #[test]
    fn genuinely_new_rule_still_passes() {
        let existing = "\
- **Always quote SSH commands**: Use double quotes.
  ADDED 2026-06-11 (session aaa): confirmed.";

        let new_entry = "\
- **Never delete production data**: Always verify environment before destructive ops.";

        let result = filter_duplicate_append(existing, new_entry);
        assert!(
            matches!(result, AppendDedup::AllNew),
            "Different rule should pass through, got: {:?}",
            result,
        );
    }

    #[test]
    fn repeat_prefix_caught() {
        let existing = "- **Some rule**: Description.\n  ADDED 2026-06-01 (session x): note.";
        let new_entry =
            "- **Some rule**: Description.\n  REPEAT 2026-06-14 (session y): happened again.";

        let result = filter_duplicate_append(existing, new_entry);
        assert!(
            matches!(result, AppendDedup::AllDuplicate),
            "REPEAT prefix with different date should be caught, got: {:?}",
            result,
        );
    }

    #[test]
    fn single_line_short_content_not_normalized() {
        // Normalization overlap requires >= 2 lines to avoid false positives
        let existing = "some rule here";
        let new_entry = "completely different thing 2026-06-14";

        let result = filter_duplicate_append(existing, new_entry);
        assert!(
            matches!(result, AppendDedup::AllNew),
            "Short single-line content should not trigger normalization, got: {:?}",
            result,
        );
    }
}

mod looks_like_failure_log_plain_text {
    use crate::brain::tools::self_improve::SelfImproveTool;
    use crate::brain::tools::{Tool, ToolExecutionContext};
    use serde_json::json;
    use uuid::Uuid;

    fn ctx() -> ToolExecutionContext {
        ToolExecutionContext::new(Uuid::new_v4())
    }

    #[tokio::test]
    async fn rejects_added_session_prefix() {
        let tool = SelfImproveTool;
        let result = tool
            .execute(
                json!({
                    "action": "apply",
                    "target_file": "TOOLS.md",
                    "description": "incident log",
                    "rationale": "test",
                    "content": "ADDED 2026-06-11 (session ba623fd1): Another quoting error in ssh_exec command",
                }),
                &ctx(),
            )
            .await
            .unwrap();
        assert!(
            !result.success,
            "ADDED + session prefix should be rejected, got success: {}",
            result.output,
        );
        let err = result.error.unwrap_or_default();
        assert!(
            err.contains("incident-log") || err.contains("RULES"),
            "rejection should explain rule vs incident distinction, got: {err}",
        );
    }

    #[tokio::test]
    async fn rejects_repeat_session_prefix() {
        let tool = SelfImproveTool;
        let result = tool
            .execute(
                json!({
                    "action": "apply",
                    "target_file": "SOUL.md",
                    "description": "repeat incident",
                    "rationale": "test",
                    "content": "REPEAT 2026-06-13 (session ca91e02d): SSH quoting violation happened again",
                }),
                &ctx(),
            )
            .await
            .unwrap();
        assert!(
            !result.success,
            "REPEAT + session prefix should be rejected, got success: {}",
            result.output,
        );
    }

    #[tokio::test]
    async fn allows_clean_rule_with_date_context() {
        // A clean rule that mentions a date in context (not as ADDED/REPEAT prefix)
        // should still pass. This is the "good" version of documenting recurrence.
        let tool = SelfImproveTool;
        let result = tool
            .execute(
                json!({
                    "action": "apply",
                    "target_file": "TOOLS.md",
                    "description": "quoting rule",
                    "rationale": "test",
                    "content": "- **Always quote SSH commands**: Use double quotes for inner commands. Violations: 4, last: 2026-06-11",
                }),
                &ctx(),
            )
            .await
            .unwrap();
        // This should succeed (it's a clean rule with inline counter, not an incident log)
        // But it may fail for other reasons (TOOLS.md doesn't exist in test env).
        // The key thing is it should NOT fail with the incident-log rejection.
        if let Some(ref err) = result.error {
            assert!(
                !err.contains("incident-log"),
                "Clean rule with inline counter should not be rejected as incident log: {err}",
            );
        }
    }
}

mod real_world_scenario {
    use crate::brain::tools::brain_file_safety::{AppendDedup, filter_duplicate_append};

    #[test]
    fn soul_md_16_entries_would_be_blocked() {
        // Simulates the actual scenario from issue #197: SOUL.md has a rule
        // with 16 ADDED entries, and the 17th would be blocked.
        let mut existing = String::from(
            "- **Always quote SSH commands**: When passing commands to ssh_exec, wrap the\n  \
             inner command in double quotes.\n",
        );
        // Add 5 accumulated ADDED entries (simulating the 16 from the issue)
        for i in 0..5 {
            existing.push_str(&format!(
                "  ADDED 2026-06-{:02} (session {:08x}): Another quoting error in ssh_exec.\n",
                10 + i,
                0xaaa000 + i,
            ));
        }

        // Try to add a 6th entry
        let new_entry = "\
- **Always quote SSH commands**: When passing commands to ssh_exec, wrap the
  inner command in double quotes.
  ADDED 2026-06-15 (session deadbeef): Yet another quoting error.";

        let result = filter_duplicate_append(&existing, new_entry);
        assert!(
            matches!(result, AppendDedup::AllDuplicate),
            "6th incident entry of same rule should be blocked, got: {:?}",
            result,
        );
    }

    #[test]
    fn inline_counter_format_is_clean() {
        // The "good" format after 2 entries: inline counter, no more entries.
        let existing = "\
- **Always quote SSH commands**: Use double quotes for SSH commands.
  ADDED 2026-06-11 (session aaa): First incident.
  ADDED 2026-06-12 (session bbb): Second incident.
  Violations: 4, last: 2026-06-14";

        // This is what the RSI agent should produce instead of a new ADDED entry.
        // The inline counter format should NOT be caught as an incident log.
        // It should be caught as duplicate (same rule already exists).
        let new_entry = "\
- **Always quote SSH commands**: Use double quotes for SSH commands.
  ADDED 2026-06-11 (session aaa): First incident.
  ADDED 2026-06-12 (session bbb): Second incident.
  Violations: 5, last: 2026-06-15";

        let result = filter_duplicate_append(existing, new_entry);
        assert!(
            matches!(result, AppendDedup::AllDuplicate),
            "Updated inline counter should be caught as duplicate, got: {:?}",
            result,
        );
    }
}
