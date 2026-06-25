//! Tests for issue #221: TOML 1.0 dotted-key parsing in [brain.caps].
//!
//! Verifies that `deser_caps_compat` correctly flattens nested tables
//! produced by unquoted dotted keys (e.g. `AGENTS.md = 600`) back into
//! the expected `BTreeMap<String, usize>`.

use crate::config::Config;

/// Unquoted dotted key `AGENTS.md = 600` is parsed by TOML 1.0 as a
/// nested table `{AGENTS: {md: 600}}`. The custom deserializer must
/// flatten this back to `"AGENTS.md" = 600`.
#[test]
fn unquoted_dotted_key_is_accepted() {
    let toml = r#"
[brain.caps]
AGENTS.md = 600
TOOLS.md = 400
"#;

    let config: Config = toml::from_str(toml).unwrap();
    let caps = &config.brain.caps;

    assert_eq!(caps.get("AGENTS.md"), Some(&600));
    assert_eq!(caps.get("TOOLS.md"), Some(&400));
    assert_eq!(caps.len(), 2);
}

/// Quoted keys (the documented form) still work.
#[test]
fn quoted_keys_still_work() {
    let toml = r#"
[brain.caps]
"AGENTS.md" = 600
"TOOLS.md" = 400
"#;

    let config: Config = toml::from_str(toml).unwrap();
    let caps = &config.brain.caps;

    assert_eq!(caps.get("AGENTS.md"), Some(&600));
    assert_eq!(caps.get("TOOLS.md"), Some(&400));
}

/// Mixed quoted and unquoted keys in the same section.
#[test]
fn mixed_quoted_and_unquoted_keys() {
    let toml = r#"
[brain.caps]
AGENTS.md = 600
"SOUL.md" = 300
MEMORY.md = 800
"#;

    let config: Config = toml::from_str(toml).unwrap();
    let caps = &config.brain.caps;

    assert_eq!(caps.get("AGENTS.md"), Some(&600));
    assert_eq!(caps.get("SOUL.md"), Some(&300));
    assert_eq!(caps.get("MEMORY.md"), Some(&800));
    assert_eq!(caps.len(), 3);
}

/// Empty caps section parses to empty map.
#[test]
fn empty_caps_section() {
    let toml = r#"
[brain.caps]
"#;

    let config: Config = toml::from_str(toml).unwrap();
    assert!(config.brain.caps.is_empty());
}

/// No [brain.caps] section at all defaults to empty map.
#[test]
fn no_caps_section_defaults_to_empty() {
    let toml = r#"
[brain]
strip_empty_sections = true
"#;

    let config: Config = toml::from_str(toml).unwrap();
    assert!(config.brain.caps.is_empty());
}

/// `cap_for` resolves correctly with dotted-key parsed caps.
#[test]
fn cap_for_resolves_with_dotted_keys() {
    let toml = r#"
[brain.caps]
AGENTS.md = 600
"#;

    let config: Config = toml::from_str(toml).unwrap();
    assert_eq!(config.brain.cap_for("AGENTS.md"), 600);
    assert_eq!(config.brain.cap_for("SOUL.md"), 500); // falls back to default_cap
}
