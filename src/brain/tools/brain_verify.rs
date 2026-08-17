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
pub(crate) struct BrainVerifyConfig {
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
pub(crate) fn pattern_matches(pattern: &str, content: &str) -> bool {
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
pub(crate) fn verify_brain_file_with_config(
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
pub(crate) fn orient_gate_decision(
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
