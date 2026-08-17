//! `[memory]` external-path config parsing (#1051).
//!
//! Moved out of `src/config/types.rs`: tests live under `src/tests/`,
//! never inline beside the type they exercise (#1076).

use crate::config::types::{ExtraPath, MemoryConfig};

fn parse(s: &str) -> MemoryConfig {
    toml::from_str(s).expect("valid [memory] TOML")
}

#[test]
fn bare_string_entry_parses_with_default_md_pattern() {
    let cfg = parse(r#"extra_paths = ["/home/u/notes"]"#);
    assert_eq!(cfg.extra_paths.len(), 1);
    assert_eq!(cfg.extra_paths[0].path(), "/home/u/notes");
    assert_eq!(cfg.extra_paths[0].pattern(), "**/*.md");
    assert!(matches!(cfg.extra_paths[0], ExtraPath::Simple(_)));
}

#[test]
fn table_entry_parses_with_explicit_pattern() {
    let cfg = parse(
        r#"[[extra_paths]]
path = "/home/u/docs"
pattern = "**/*.txt"
"#,
    );
    assert_eq!(cfg.extra_paths.len(), 1);
    assert_eq!(cfg.extra_paths[0].path(), "/home/u/docs");
    assert_eq!(cfg.extra_paths[0].pattern(), "**/*.txt");
    assert!(matches!(cfg.extra_paths[0], ExtraPath::WithPattern { .. }));
}

#[test]
fn table_entry_without_pattern_defaults_to_md() {
    let cfg = parse(
        r#"[[extra_paths]]
path = "/home/u/docs"
"#,
    );
    assert_eq!(cfg.extra_paths[0].pattern(), "**/*.md");
}

#[test]
fn mixed_forms_parse_together() {
    let cfg = parse(r#"extra_paths = ["/a", { path = "/b", pattern = "**/*.org" }]"#);
    assert_eq!(cfg.extra_paths.len(), 2);
    assert_eq!(cfg.extra_paths[0].path(), "/a");
    assert_eq!(cfg.extra_paths[1].path(), "/b");
    assert_eq!(cfg.extra_paths[1].pattern(), "**/*.org");
}

#[test]
fn defaults_are_secure_and_sane() {
    let cfg = parse("");
    assert!(cfg.extra_paths.is_empty(), "no paths by default");
    assert!(
        !cfg.external_allowed_in_shared,
        "session gate must default to DENY (#1051 security boundary)"
    );
    assert_eq!(cfg.sweep_interval_secs, 300);
    let excl = &cfg.exclude;
    for secret in [".env*", "*.key", "*.pem", ".ssh/**", "*credential*"] {
        assert!(
            excl.iter().any(|e| e == secret),
            "missing secret exclude {secret}"
        );
    }
    for noise in [
        ".git",
        "node_modules",
        "target",
        "dist",
        "build",
        "vendor",
        "__pycache__",
    ] {
        assert!(
            excl.iter().any(|e| e == noise),
            "missing noise exclude {noise}"
        );
    }
}

#[test]
fn explicit_exclude_overrides_defaults() {
    let cfg = parse(r#"exclude = ["*.md"]"#);
    assert_eq!(cfg.exclude, vec!["*.md".to_string()]);
}

#[test]
fn gate_and_interval_are_overridable() {
    let cfg = parse("external_allowed_in_shared = true\nsweep_interval_secs = 60");
    assert!(cfg.external_allowed_in_shared);
    assert_eq!(cfg.sweep_interval_secs, 60);
}
