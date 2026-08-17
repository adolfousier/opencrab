//! brain_verify tool.
//!
//! Moved out of `src/brain/tools/brain_verify.rs`: tests live under `src/tests/`,
//! never inline beside the logic they exercise (#1076).

use crate::brain::tools::brain_verify::*;

/// Build a test config from a TOML string. Tests are self-contained
/// and do NOT depend on the live brain_verify.toml.
fn test_config(toml_str: &str) -> BrainVerifyConfig {
    toml::from_str(toml_str).expect("test TOML must parse")
}

const AGENTS_TOML: &str = r#"
[[required]]
file = "AGENTS.md"
pattern = "Reports use rich markdown tables"
why = "Report Format Hard Rule"

[[required]]
file = "AGENTS.md"
pattern = "NEVER add.*Co-authored-by"
why = "Commit attribution: user is sole author"

[[required]]
file = "AGENTS.md"
pattern = "NEVER push to main without explicit"
why = "Git push safety"

[[required]]
file = "AGENTS.md"
pattern = "NEVER use.*git revert"
why = "Git revert creates new commits"

[[required]]
file = "AGENTS.md"
pattern = "NEVER start.*draft.*redo a release"
why = "Release safety"

[[required]]
file = "AGENTS.md"
pattern = "NEVER delete.*disable.*cron"
why = "Cron safety"

[[contradictions]]
pattern_a = "(?i)no.*markdown.*table"
pattern_b = "(?i)rich markdown table"
message = "'no markdown tables' vs 'use rich markdown tables'"

[[contradictions]]
pattern_a = "(?i)never.*push"
pattern_b = "(?i)always.*push"
message = "'never push' vs 'always push'"
"#;

#[test]
fn test_simple_substring_match() {
    assert!(pattern_matches(
        "BAN em-dashes",
        "## BAN em-dashes. ZERO TOLERANCE."
    ));
    assert!(!pattern_matches("BAN em-dashes", "No issues here"));
}

#[test]
fn test_ordered_part_match() {
    let text = "NEVER start, draft, or redo a release unless explicitly asked.";
    assert!(pattern_matches("NEVER start.*draft.*redo a release", text));
    // Wrong order
    assert!(!pattern_matches("draft.*NEVER start", text));
}

#[test]
fn test_case_insensitive() {
    let text = "Reports use rich markdown tables for tabular data.";
    assert!(pattern_matches("(?i)reports use rich markdown table", text));
    assert!(pattern_matches("reports use rich", text));
}

#[test]
fn test_pattern_with_glob() {
    let text = "NEVER push to main without explicit user approval.";
    assert!(pattern_matches("NEVER push.*without explicit", text));
    assert!(!pattern_matches(
        "NEVER push.*without approval.*extra",
        text
    ));
}

#[test]
fn test_verify_brain_file_required() {
    let config = test_config(AGENTS_TOML);
    // AGENTS.md rule: "Reports use rich markdown tables"
    let content_no_rule = "Some content without the required rule.";
    let violations = verify_brain_file_with_config("AGENTS.md", content_no_rule, &config);
    assert!(
        violations.iter().any(|v| v.contains("Reports use rich")),
        "Should detect missing required rule. Violations: {:?}",
        violations
    );
}

#[test]
fn test_verify_brain_file_no_violations() {
    let config = test_config(AGENTS_TOML);
    // Content with all AGENTS.md rules present
    let content = r#"
Reports use rich markdown tables for structured data.
NEVER add Co-authored-by trailers to commits.
NEVER push to main without explicit user approval.
NEVER use git revert.
NEVER start, draft, or redo a release unless explicitly asked.
NEVER delete or disable cron jobs without approval.
"#;
    let violations = verify_brain_file_with_config("AGENTS.md", content, &config);
    // Should have no required-rule violations (contradictions may or may not fire)
    let required_violations: Vec<_> = violations
        .iter()
        .filter(|v| v.contains("Required rule missing"))
        .collect();
    assert!(
        required_violations.is_empty(),
        "Expected no required violations, got: {:?}",
        required_violations
    );
}

#[test]
fn test_contradiction_detection() {
    let config = test_config(AGENTS_TOML);
    // Both patterns in the SAME entry → contradiction detected
    let content = "No markdown tables. Use rich markdown tables for reports.";
    let violations = verify_brain_file_with_config("AGENTS.md", content, &config);
    assert!(
        violations.iter().any(|v| v.contains("Contradiction")),
        "Should detect contradiction in same entry. Violations: {:?}",
        violations
    );
}

#[test]
fn test_contradiction_scoped_per_entry() {
    let config = test_config(AGENTS_TOML);
    // Patterns in DIFFERENT entries (separated by blank line) → NO contradiction.
    // This is the #855 fix: "NEVER push" in one rule and "always push" in
    // an unrelated entry must NOT trigger a false positive.
    let content = "NEVER push to main without explicit user approval.\n\nAlways push after tests pass and the user says go.";
    let violations = verify_brain_file_with_config("MEMORY.md", content, &config);
    let contradictions: Vec<_> = violations
        .iter()
        .filter(|v| v.contains("Contradiction"))
        .collect();
    assert!(
        contradictions.is_empty(),
        "Patterns in separate entries must NOT trigger contradiction. Got: {:?}",
        contradictions
    );
}

#[test]
fn test_contradiction_same_entry_still_fires() {
    let config = test_config(AGENTS_TOML);
    // Both patterns in the SAME paragraph → contradiction still detected
    let content = "Never push anything. Always push everything.";
    let violations = verify_brain_file_with_config("MEMORY.md", content, &config);
    assert!(
        violations.iter().any(|v| v.contains("Contradiction")),
        "Same-entry contradiction must still fire. Violations: {:?}",
        violations
    );
}

#[test]
fn test_wrong_file_ignored() {
    let config = test_config(AGENTS_TOML);
    // Rules for AGENTS.md should not trigger on MEMORY.md
    let content = "Some content without any rules.";
    let violations = verify_brain_file_with_config("MEMORY.md", content, &config);
    let agents_violations: Vec<_> = violations
        .iter()
        .filter(|v| v.contains("AGENTS.md"))
        .collect();
    assert!(
        agents_violations.is_empty(),
        "AGENTS.md rules should not apply to MEMORY.md"
    );
}

// ---- Orient gate (orient_gate_decision) — #881 ----

#[test]
fn orient_gate_clean_content_is_allowed() {
    let config = test_config(AGENTS_TOML);
    // Content satisfying every AGENTS.md required rule → Allow.
    let content = r#"
Reports use rich markdown tables for structured data.
NEVER add Co-authored-by trailers to commits.
NEVER push to main without explicit user approval.
NEVER use git revert.
NEVER start, draft, or redo a release unless explicitly asked.
NEVER delete or disable cron jobs without approval.
"#;
    match orient_gate_decision("AGENTS.md", content, Some(&config), true) {
        GateDecision::Allow => {}
        other => panic!("expected Allow, got {other:?}"),
    }
}

#[test]
fn orient_gate_strips_required_rule_is_rejected() {
    let config = test_config(AGENTS_TOML);
    // Missing "Reports use rich markdown tables" → Reject.
    let content = "NEVER add Co-authored-by. NEVER push to main without explicit user approval.";
    match orient_gate_decision("AGENTS.md", content, Some(&config), true) {
        GateDecision::Reject(msg) => assert!(
            msg.contains("Required rule missing"),
            "should name the missing required rule, got: {msg}"
        ),
        other => panic!("expected Reject, got {other:?}"),
    }
}

#[test]
fn orient_gate_introduces_contradiction_is_rejected() {
    let config = test_config(AGENTS_TOML);
    // Both contradicting patterns in the SAME paragraph → Reject.
    let content = "Never push anything. Always push everything.";
    match orient_gate_decision("MEMORY.md", content, Some(&config), true) {
        GateDecision::Reject(msg) => assert!(
            msg.contains("Contradiction"),
            "should flag the contradiction, got: {msg}"
        ),
        other => panic!("expected Reject, got {other:?}"),
    }
}

#[test]
fn orient_gate_hard_fails_when_no_belief_base() {
    // Autonomous path (hard_fail = true) with NO belief base → hard Reject.
    // This is the fix for the silent no-op: a gate with no rules looks
    // enforced but checks nothing, so it must refuse rather than allow.
    match orient_gate_decision("AGENTS.md", "anything", None, true) {
        GateDecision::Reject(msg) => assert!(
            msg.contains("not loaded"),
            "should explain the missing belief base, got: {msg}"
        ),
        other => panic!("expected Reject on missing belief base, got {other:?}"),
    }
}

#[test]
fn orient_gate_user_path_allows_when_no_belief_base() {
    // User-facing path (hard_fail = false) keeps legacy graceful behavior:
    // no belief base → Allow (no-op), not a hard reject.
    match orient_gate_decision("AGENTS.md", "anything", None, false) {
        GateDecision::Allow => {}
        other => panic!("expected Allow on user path with no config, got {other:?}"),
    }
}
