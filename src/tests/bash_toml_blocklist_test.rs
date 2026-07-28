//! TOML blocklist layer precedence and matching (#851).
//!
//! The blocklist has two layers: a hardcoded Rust floor that runs first and
//! cannot be weakened, and this runtime TOML layer on top. The only construct
//! that turns a block into an allow is an override, which makes it the surface
//! most worth pinning.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::brain::tools::bash::{TomlBlockRule, TomlBlocklist, TomlOverride, evaluate_blocklist};

fn rule(id: &str, patterns: &[&str]) -> TomlBlockRule {
    TomlBlockRule {
        id: id.to_string(),
        category: "test".to_string(),
        severity: "block".to_string(),
        reason: format!("{id} is not allowed"),
        patterns: patterns.iter().map(|p| p.to_string()).collect(),
    }
}

fn overr(id: &str, patterns: &[&str]) -> TomlOverride {
    TomlOverride {
        id: id.to_string(),
        reason: "test override".to_string(),
        patterns: patterns.iter().map(|p| p.to_string()).collect(),
    }
}

fn blocklist(rules: Vec<TomlBlockRule>, overrides: Vec<TomlOverride>) -> TomlBlocklist {
    TomlBlocklist {
        meta: None,
        rules,
        overrides,
    }
}

#[test]
fn a_matching_rule_blocks_and_names_itself() {
    let bl = blocklist(vec![rule("no-curl-pipe", &["curl | sh"])], vec![]);
    let blocked = evaluate_blocklist(&bl, "curl | sh").expect("must block");
    assert!(
        blocked.contains("no-curl-pipe"),
        "the reason must name the rule so the source is traceable: {blocked}"
    );
}

#[test]
fn an_unmatched_command_is_allowed() {
    let bl = blocklist(vec![rule("no-curl-pipe", &["curl | sh"])], vec![]);
    assert_eq!(evaluate_blocklist(&bl, "cargo test"), None);
}

#[test]
fn an_override_wins_over_a_matching_rule() {
    // The whole point of overrides, and the only allow-path in this layer.
    let bl = blocklist(
        vec![rule("no-rm", &["rm -rf"])],
        vec![overr("allow-build-dir", &["rm -rf target"])],
    );
    assert_eq!(evaluate_blocklist(&bl, "rm -rf target"), None);
}

#[test]
fn an_override_does_not_leak_to_other_commands() {
    // An override must relax exactly what it names, not the rule wholesale.
    let bl = blocklist(
        vec![rule("no-rm", &["rm -rf"])],
        vec![overr("allow-build-dir", &["rm -rf target"])],
    );
    assert!(
        evaluate_blocklist(&bl, "rm -rf /").is_some(),
        "the override widened past its own pattern"
    );
}

#[test]
fn only_block_severity_blocks() {
    // A warn-level rule must not silently behave like a block.
    let mut warn_rule = rule("noisy", &["cargo build"]);
    warn_rule.severity = "warn".to_string();
    let bl = blocklist(vec![warn_rule], vec![]);
    assert_eq!(evaluate_blocklist(&bl, "cargo build"), None);
}

#[test]
fn a_rule_with_no_patterns_blocks_nothing() {
    // Guards against an empty pattern list matching every command.
    let bl = blocklist(vec![rule("empty", &[])], vec![]);
    assert_eq!(evaluate_blocklist(&bl, "anything at all"), None);
}

#[test]
fn matching_is_case_insensitive() {
    let bl = blocklist(vec![rule("no-rm", &["rm -rf"])], vec![]);
    assert!(evaluate_blocklist(&bl, "RM -RF /tmp/x").is_some());
}

#[test]
fn whitespace_is_collapsed_before_matching() {
    // Padding a command with spaces or newlines must not evade a rule.
    let bl = blocklist(vec![rule("no-rm", &["rm -rf"])], vec![]);
    for cmd in ["rm    -rf /x", "rm\t-rf /x", "rm\n-rf /x"] {
        assert!(
            evaluate_blocklist(&bl, cmd).is_some(),
            "whitespace evaded the rule: {cmd:?}"
        );
    }
}

#[test]
fn matching_is_substring_not_word_boundary() {
    // Documents a real sharp edge rather than asserting it is good: a short
    // pattern over-blocks. If this ever starts failing, matching became
    // boundary-aware and the docs on evaluate_blocklist need updating.
    let bl = blocklist(vec![rule("short", &["rm"])], vec![]);
    assert!(
        evaluate_blocklist(&bl, "npm run format").is_some(),
        "'rm' no longer matches inside 'format' — matching semantics changed"
    );
}

#[test]
fn an_empty_blocklist_allows_everything() {
    // The state after a missing or unparseable file, once it falls through.
    let bl = blocklist(vec![], vec![]);
    assert_eq!(evaluate_blocklist(&bl, "rm -rf /"), None);
}

#[test]
fn the_first_matching_rule_is_reported() {
    // Deterministic attribution when two rules cover the same command.
    let bl = blocklist(
        vec![rule("first", &["rm -rf"]), rule("second", &["rm"])],
        vec![],
    );
    let blocked = evaluate_blocklist(&bl, "rm -rf /").expect("must block");
    assert!(
        blocked.contains("first"),
        "unexpected attribution: {blocked}"
    );
}
