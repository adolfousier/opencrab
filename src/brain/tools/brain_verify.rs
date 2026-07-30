//! Brain File Verification Gate
//!
//! After every write to a protected brain file, verifies the new content
//! against `~/.opencrabs/safety/brain_verify.toml` rules. If required rules
//! are missing or contradictions are detected, the write is rejected.
//!
//! Design:
//! - TOML-driven: rules editable at runtime, no rebuild needed
//! - Profile-aware: resolves the belief base from the active profile home, so
//!   the gate always checks the content against the home being written (#881)
//! - Graceful fallback (user path): missing/invalid TOML = no verification
//! - Ordered-part matching: patterns with `.*` split into ordered substrings

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct BrainVerifyConfig {
    #[serde(default)]
    required: Vec<RequiredRule>,
    #[serde(default)]
    contradictions: Vec<ContradictionRule>,
}

#[derive(Debug, Deserialize)]
struct RequiredRule {
    file: String,
    pattern: String,
    #[allow(dead_code)]
    why: String,
}

#[derive(Debug, Deserialize)]
struct ContradictionRule {
    pattern_a: String,
    pattern_b: String,
    message: String,
}

/// Load the brain verification belief base from the ACTIVE profile home.
///
/// Resolves via `resolve_profile_home()` — the same home `self_improve` and
/// `write_opencrabs_file` write to — so the gate always checks content against
/// the belief base that governs the home being written. Returns `None` if the
/// file is missing, unparseable, or unreadable.
///
/// Not cached: the active profile can change at runtime (profile switch, or
/// test isolation via `with_profile_home_async`), and a process-global cache
/// would serve a stale belief base to a different home. The TOML is ~1-2 KB and
/// this runs once per write, not in a hot loop. (#881)
fn brain_verify_config() -> Option<BrainVerifyConfig> {
    let home = crate::config::profile::resolve_profile_home();
    let path = home.join("safety").join("brain_verify.toml");
    if !path.exists() {
        tracing::debug!("No brain verify TOML at {}", path.display());
        return None;
    }
    match std::fs::read_to_string(&path) {
        Ok(content) => match toml::from_str::<BrainVerifyConfig>(&content) {
            Ok(cfg) => {
                tracing::debug!(
                    "Loaded brain verify config from {}: {} required rules, {} contradiction checks",
                    path.display(),
                    cfg.required.len(),
                    cfg.contradictions.len()
                );
                Some(cfg)
            }
            Err(e) => {
                tracing::warn!("Brain verify TOML parse error at {}: {}", path.display(), e);
                None
            }
        },
        Err(e) => {
            tracing::warn!("Brain verify TOML read error at {}: {}", path.display(), e);
            None
        }
    }
}

/// Check if a pattern matches content using ordered-part matching.
///
/// Strips `(?i)` prefix (always case-insensitive). Splits on `.*` to get
/// ordered parts. Returns true if ALL parts appear in the text in order.
///
/// Example: "NEVER start.*draft.*redo a release" matches text containing
/// "NEVER start" then later "draft" then later "redo a release".
fn pattern_matches(pattern: &str, content: &str) -> bool {
    // Strip (?i) prefix if present (we always do case-insensitive)
    let clean = if let Some(rest) = pattern.strip_prefix("(?i)") {
        rest
    } else {
        pattern
    };

    let content_lower = content.to_lowercase();
    let parts: Vec<&str> = clean.split(".*").collect();

    if parts.len() == 1 {
        // Simple substring match
        return content_lower.contains(&parts[0].to_lowercase());
    }

    // Ordered-part matching: each part must appear after the previous one
    let mut search_from = 0usize;
    for part in &parts {
        let part_lower = part.to_lowercase();
        match content_lower[search_from..].find(&part_lower) {
            Some(pos) => search_from += pos + part_lower.len(),
            None => return false,
        }
    }
    true
}

/// Verify brain file content against all applicable rules.
///
/// Returns a list of violation descriptions. Empty list = all checks pass.
/// Only checks rules where `file` matches `file_name` (e.g. "AGENTS.md").
pub fn verify_brain_file(file_name: &str, content: &str) -> Vec<String> {
    let Some(config) = brain_verify_config() else {
        return vec![]; // No TOML = no verification (graceful fallback)
    };
    verify_brain_file_with_config(file_name, content, &config)
}

/// Verify brain file content against a specific config.
/// Used by tests to be self-contained (not dependent on live TOML).
fn verify_brain_file_with_config(
    file_name: &str,
    content: &str,
    config: &BrainVerifyConfig,
) -> Vec<String> {
    let mut violations = Vec::new();

    // Check required rules for this file
    for rule in &config.required {
        if rule.file == file_name && !pattern_matches(&rule.pattern, content) {
            violations.push(format!(
                "Required rule missing in {}: \"{}\" ({})",
                file_name, rule.pattern, rule.why
            ));
        }
    }

    // Check contradiction pairs for this file.
    // Contradictions apply to all brain files (no file field in TOML).
    // Scoped per-entry: both patterns must match within the SAME entry
    // (paragraph/section), not across the entire file. This prevents
    // false positives where unrelated entries happen to contain both
    // patterns (e.g. "NEVER push" in one rule and "always push" in another).
    let entries: Vec<&str> = content.split("\n\n").collect();
    for contra in &config.contradictions {
        let contradiction_in_entry = entries.iter().any(|entry| {
            pattern_matches(&contra.pattern_a, entry) && pattern_matches(&contra.pattern_b, entry)
        });
        if contradiction_in_entry {
            violations.push(format!(
                "Contradiction detected in {}: {}",
                file_name, contra.message
            ));
        }
    }

    violations
}

/// OODA **Orient** gate decision: does the proposed brain-file content pass
/// verification against the belief base, or must the write be rejected?
///
/// `None` config = no belief base loaded. For the autonomous RSI path
/// (`hard_fail_on_no_config = true`) that is a hard reject — a gate with no
/// rules to check is worse than no gate, because it *looks* enforced. The
/// user-facing path passes `false` to keep the legacy graceful (no-op) behavior.
#[derive(Debug)]
pub enum GateDecision {
    Allow,
    Reject(String),
}

/// Pure Orient-gate decision over an explicit config. Deterministic — this is
/// what unit tests exercise; the live path uses `orient_gate_decision_active`.
fn orient_gate_decision(
    file_name: &str,
    proposed_content: &str,
    config: Option<&BrainVerifyConfig>,
    hard_fail_on_no_config: bool,
) -> GateDecision {
    match config {
        None => {
            if hard_fail_on_no_config {
                GateDecision::Reject(
                    "Brain verification belief base (brain_verify.toml) is not loaded. \
                     Autonomous writes are blocked until it exists — a gate with no rules \
                     looks enforced but checks nothing."
                        .to_string(),
                )
            } else {
                GateDecision::Allow
            }
        }
        Some(cfg) => {
            let violations = verify_brain_file_with_config(file_name, proposed_content, cfg);
            if violations.is_empty() {
                GateDecision::Allow
            } else {
                GateDecision::Reject(format!(
                    "Brain file verification failed for {}: {}",
                    file_name,
                    violations.join("; ")
                ))
            }
        }
    }
}

/// Orient gate bound to the live (loaded) belief base. Used by the autonomous
/// `self_improve` path: hard-fails when no belief base is loaded, else verifies
/// the proposed content and rejects on any violation. (#881)
pub fn orient_gate_decision_active(file_name: &str, proposed_content: &str) -> GateDecision {
    orient_gate_decision(
        file_name,
        proposed_content,
        brain_verify_config().as_ref(),
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let content =
            "NEVER add Co-authored-by. NEVER push to main without explicit user approval.";
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
}
