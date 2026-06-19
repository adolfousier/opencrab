//! Unit tests for the config auto-repair (`crate::config::repair`).
//!
//! The repairer closes unterminated arrays / inline tables left by a hand edit,
//! gated on the result re-parsing as valid TOML. Anything it can't fix cleanly
//! returns None so the caller falls back to last-known-good.

use crate::config::repair::repair_toml;

fn parses(s: &str) -> bool {
    toml::from_str::<toml::Value>(s).is_ok()
}

#[test]
fn already_valid_config_is_left_alone() {
    let ok = r#"
[agent]
approval_policy = "auto-always"
models = ["a", "b", "c"]
"#;
    assert!(repair_toml(ok).is_none(), "valid config needs no repair");
}

#[test]
fn unterminated_array_before_header_is_closed() {
    // The exact shape that took config loading down: a models array that lost
    // its closing ']' right before the next table header.
    let broken = r#"
[providers.custom.a]
models = ["x", "y", "z"

[providers.custom.b]
default_model = "m"
"#;
    assert!(
        !parses(broken),
        "precondition: broken config must not parse"
    );
    let (fixed, fixes) = repair_toml(broken).expect("should repair an unterminated array");
    assert!(parses(&fixed), "repaired config must parse");
    assert!(!fixes.is_empty());
    // The second provider section must survive intact.
    let v: toml::Value = toml::from_str(&fixed).unwrap();
    assert!(v["providers"]["custom"]["b"].get("default_model").is_some());
}

#[test]
fn unterminated_array_at_eof_is_closed() {
    let broken = r#"
[agent]
approval_policy = "auto-always"
models = ["x", "y"
"#;
    assert!(!parses(broken));
    let (fixed, _) = repair_toml(broken).expect("should close array at EOF");
    assert!(parses(&fixed));
    let v: toml::Value = toml::from_str(&fixed).unwrap();
    assert_eq!(v["agent"]["approval_policy"].as_str(), Some("auto-always"));
}

#[test]
fn unterminated_inline_table_is_not_repaired() {
    // TOML forbids multi-line inline tables, so a dangling `{` can't be closed
    // across a line break safely — the repairer must abort and let the caller
    // fall back to last-known-good rather than emit invalid TOML.
    let broken = r#"
[server]
opts = { retries = 3, timeout = 30

[other]
x = 1
"#;
    assert!(!parses(broken));
    assert!(
        repair_toml(broken).is_none(),
        "an unterminated inline table must not be auto-repaired"
    );
}

#[test]
fn valid_multiline_and_nested_arrays_are_not_touched() {
    // A legitimately multi-line array (and nested arrays) already parses, so the
    // repairer must return None — never rewrite working config.
    let ok = r#"
[x]
list = [
  "a",
  "b",
]
matrix = [
  [1, 2],
  [3, 4],
]
servers = [
  { name = "a" },
  { name = "b" },
]
"#;
    assert!(parses(ok));
    assert!(repair_toml(ok).is_none());
}

#[test]
fn unfixable_syntax_error_returns_none() {
    // A stray token that isn't a delimiter problem can't be safely repaired —
    // the caller must fall back to last-known-good, not guess.
    let broken = r#"
[agent]
approval_policy = "auto-always" / oops
"#;
    assert!(!parses(broken));
    assert!(
        repair_toml(broken).is_none(),
        "must not fabricate a fix for a non-delimiter error"
    );
}

#[test]
fn brackets_inside_strings_are_not_miscounted() {
    // A '[' inside a string value must not be treated as an open array.
    let ok = r#"
[a]
note = "this [is] fine"
url = "http://example.com/x"
"#;
    assert!(parses(ok));
    assert!(repair_toml(ok).is_none());
}
