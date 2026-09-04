//! End-to-end write_config behaviour against a real config.toml in a
//! tempdir home (#83): the compiled `Config` struct is the registry.
//!
//! - sections the struct knows but the config tool only renders at the first
//!   level (`[memory]`, seeded into the home) write cleanly — the registry
//!   recognises them, no warning needed;
//! - genuinely unknown sections and keys are refused with the file left
//!   byte-identical;
//! - the post-write TOML parse guard still hard-denies schema-breaking
//!   writes (#714, untouched by #83).
//!
//! Tool-level tests use a sync poll of the `execute` future: the write path
//! never awaits, and a tokio runtime's task-local home override would not
//! survive into a freshly created runtime's task.

use std::path::{Path, PathBuf};
use std::task::{Context, Poll};

use crate::brain::tools::config_tool::ConfigTool;
use crate::brain::tools::{Tool, ToolExecutionContext, ToolResult};
use crate::config::Config;
use crate::config::profile::with_home_override;

/// A tempdir laid out as an `.opencrabs` home holding a seeded config.toml
/// whose `[memory]` section is real but unregistered by any config tool.
fn seeded_home() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let opencrabs = dir.path().join(".opencrabs");
    std::fs::create_dir_all(&opencrabs).expect("create .opencrabs");
    std::fs::write(
        opencrabs.join("config.toml"),
        "[memory]\nvector_enabled = false\n",
    )
    .expect("write config");
    std::fs::write(opencrabs.join("keys.toml"), b"").expect("write keys");
    (dir, opencrabs)
}

fn config_content(home: &Path) -> String {
    std::fs::read_to_string(home.join("config.toml")).expect("config.toml readable")
}

// ----------------------------------------------------------- write path --

/// The #83 live incident: `[memory]` is a real struct section that no config
/// tool renders — writing to it must pass with no warning and preserve the
/// existing keys.
#[test]
fn existing_unregistered_section_writes_without_a_warning() {
    let (_temp, home) = seeded_home();
    with_home_override(home.clone(), || {
        Config::write_key("memory", "extra_paths", "[\"/srv/docs\"]")
            .expect("struct-known section writes");
        let doc: toml_edit::DocumentMut = config_content(&home).parse().expect("valid TOML");
        assert_eq!(
            doc["memory"]["vector_enabled"].as_bool(),
            Some(false),
            "existing key preserved"
        );
        let arr = doc["memory"]["extra_paths"]
            .as_array()
            .expect("array written");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr.get(0).and_then(|v| v.as_str()), Some("/srv/docs"));
    });
}

/// A section the tool does render keeps working.
#[test]
fn registered_section_writes_cleanly() {
    let (_temp, home) = seeded_home();
    with_home_override(home.clone(), || {
        Config::write_key("agent", "approval_policy", "auto-always").expect("writes");
        let doc: toml_edit::DocumentMut = config_content(&home).parse().expect("valid TOML");
        assert_eq!(
            doc["agent"]["approval_policy"].as_str(),
            Some("auto-always")
        );
    });
}

#[test]
fn unknown_new_section_is_denied_and_the_file_untouched() {
    let (_temp, home) = seeded_home();
    with_home_override(home.clone(), || {
        let err = Config::write_key("opencode", "base_url", "https://x")
            .expect_err("unknown section must be denied");
        assert!(err.to_string().contains("opencode"), "{err}");
        assert_eq!(
            config_content(&home),
            "[memory]\nvector_enabled = false\n",
            "denied write must leave the file byte-identical"
        );
    });
}

#[test]
fn unknown_key_under_known_section_is_denied() {
    let (_temp, home) = seeded_home();
    with_home_override(home.clone(), || {
        let err = Config::write_key("agent", "bogus_leaf", "true")
            .expect_err("unknown key under a known section must be denied");
        assert!(err.to_string().contains("agent.bogus_leaf"), "{err}");
        assert_eq!(config_content(&home), "[memory]\nvector_enabled = false\n");
    });
}

/// The #714 parse guard still hard-denies a write that would break loading;
/// the schema guard runs after it, so this keeps the classic message.
#[test]
fn the_parse_guard_still_hard_denies_a_schema_breaking_write() {
    let (_temp, home) = seeded_home();
    with_home_override(home.clone(), || {
        let err = Config::write_key("memory", "vector_enabled", "not-a-bool")
            .expect_err("schema-breaking write must be denied");
        assert!(
            err.to_string().contains("would make config.toml invalid"),
            "{err}"
        );
        assert_eq!(config_content(&home), "[memory]\nvector_enabled = false\n");
    });
}

/// The #87 incident case: `extra_paths` is an array-of-tables. The old
/// write_key dropped non-scalar array elements silently (the file ended up
/// with `extra_paths = []`); now each object survives as an inline table and
/// `Config::load()` sees the real path/pattern pair.
#[test]
fn array_of_tables_round_trips_the_87_incident_case() {
    let (_temp, home) = seeded_home();
    with_home_override(home.clone(), || {
        let written = Config::write_key(
            "memory",
            "extra_paths",
            r#"[{"path": "/root/opencrabs/src", "pattern": "**/*.rs"}]"#,
        )
        .expect("array-of-tables writes");
        assert!(
            written.contains("path = \"/root/opencrabs/src\""),
            "echo shows the object actually written: {written}"
        );

        let doc: toml_edit::DocumentMut = config_content(&home).parse().expect("valid TOML");
        let arr = doc["memory"]["extra_paths"]
            .as_array()
            .expect("array written");
        assert_eq!(arr.len(), 1);
        let entry = arr
            .get(0)
            .and_then(|v| v.as_inline_table())
            .expect("array-of-tables element is an inline table");
        assert_eq!(
            entry.get("path").and_then(|v| v.as_str()),
            Some("/root/opencrabs/src")
        );
        assert_eq!(
            entry.get("pattern").and_then(|v| v.as_str()),
            Some("**/*.rs")
        );

        let cfg = Config::load().expect("config still loads after the write");
        let paths = &cfg.memory.extra_paths;
        assert_eq!(paths.len(), 1);
        match &paths[0] {
            crate::config::ExtraPath::WithPattern { path, pattern } => {
                assert_eq!(path, "/root/opencrabs/src");
                assert_eq!(pattern, "**/*.rs");
            }
            other => panic!("expected WithPattern entry, got {other:?}"),
        }
    });
}

/// Object-shaped input becomes a real inline table, never a string literal
/// (the old `{...}` coerce-to-string behaviour, #87). Uses struct-known
/// fields (`url`, `model`) so the schema guard passes — `provider` is not an
/// `EmbeddingConfig` field.
#[test]
fn object_input_writes_an_inline_table() {
    let (_temp, home) = seeded_home();
    with_home_override(home.clone(), || {
        let written = Config::write_key(
            "memory",
            "embedding",
            r#"{"url": "https://api.openai.com/v1", "model": "text-embedding-3-small"}"#,
        )
        .expect("object writes");
        assert!(
            written.contains("model = \"text-embedding-3-small\""),
            "{written}"
        );

        let doc: toml_edit::DocumentMut = config_content(&home).parse().expect("valid TOML");
        let tbl = doc["memory"]["embedding"]
            .as_inline_table()
            .expect("object written as inline table");
        assert_eq!(
            tbl.get("url").and_then(|v| v.as_str()),
            Some("https://api.openai.com/v1")
        );
        assert_eq!(
            tbl.get("model").and_then(|v| v.as_str()),
            Some("text-embedding-3-small")
        );
    });
}

/// Scalar control: the plain integer path still lands as a real TOML integer.
#[test]
fn scalar_control_round_trips() {
    let (_temp, home) = seeded_home();
    with_home_override(home.clone(), || {
        let written =
            Config::write_key("memory", "sweep_interval_secs", "300").expect("integer writes");
        assert_eq!(written, "300");

        let doc: toml_edit::DocumentMut = config_content(&home).parse().expect("valid TOML");
        assert_eq!(doc["memory"]["sweep_interval_secs"].as_integer(), Some(300));
    });
}

/// `null` has no TOML representation — a hard error BEFORE the file is
/// touched: the conversion fails inside write_key, write_item never runs,
/// and config.toml keeps its exact bytes.
#[test]
fn null_value_is_a_hard_error_and_file_untouched() {
    let (_temp, home) = seeded_home();
    with_home_override(home.clone(), || {
        let err = Config::write_key("memory", "extra_paths", "[null]")
            .expect_err("null must be a hard error");
        assert!(!err.to_string().is_empty(), "error carries a message");
        assert!(err.to_string().contains("null"), "{}", err);
        assert_eq!(
            config_content(&home),
            "[memory]\nvector_enabled = false\n",
            "failed write must leave the file byte-identical"
        );
    });
}

/// TOML v1.0 arrays must be homogeneous; a JSON array mixing a number and a
/// string is unrepresentable — hard error, file untouched.
#[test]
fn mixed_type_array_is_a_hard_error_and_file_untouched() {
    let (_temp, home) = seeded_home();
    with_home_override(home.clone(), || {
        let err = Config::write_key("memory", "extra_paths", r#"[1, "x"]"#)
            .expect_err("mixed-type array must be a hard error");
        assert!(err.to_string().contains("mixes types"), "{}", err);
        assert_eq!(
            config_content(&home),
            "[memory]\nvector_enabled = false\n",
            "failed write must leave the file byte-identical"
        );
    });
}

// -------------------------------------------------------- tool surface --

/// Poll an `execute` future to completion without a runtime (see module
/// doc: the write path is fully sync, and a real runtime task would lose
/// the tempdir home override).
fn execute_now(tool: &ConfigTool, input: serde_json::Value) -> ToolResult {
    let ctx = ToolExecutionContext::new(uuid::Uuid::new_v4());
    let mut fut = Box::pin(tool.execute(input, &ctx));
    let waker = futures::task::noop_waker();
    let mut cx = Context::from_waker(&waker);
    match fut.as_mut().poll(&mut cx) {
        Poll::Ready(result) => result.expect("execute returns Ok"),
        Poll::Pending => panic!("config_manager execute must not await on this path"),
    }
}

#[test]
fn the_tool_accepts_a_known_unregistered_section() {
    let (_temp, home) = seeded_home();
    with_home_override(home.clone(), || {
        let result = execute_now(
            &ConfigTool,
            serde_json::json!({
                "operation": "write_config",
                "section": "memory",
                "key": "external_allowed_in_shared",
                "value": "true"
            }),
        );
        assert!(result.success, "{}", result.error.as_deref().unwrap_or(""));
        assert!(
            result
                .output
                .contains("Set [memory].external_allowed_in_shared = true"),
            "{}",
            result.output
        );
        let doc: toml_edit::DocumentMut = config_content(&home).parse().expect("valid TOML");
        assert_eq!(
            doc["memory"]["external_allowed_in_shared"].as_bool(),
            Some(true)
        );
    });
}

#[test]
fn the_tool_still_refuses_an_unknown_new_section() {
    let (_temp, home) = seeded_home();
    with_home_override(home.clone(), || {
        let result = execute_now(
            &ConfigTool,
            serde_json::json!({
                "operation": "write_config",
                "section": "opencode",
                "key": "base_url",
                "value": "https://x"
            }),
        );
        assert!(!result.success);
        let msg = result.error.expect("error message");
        assert!(msg.contains("opencode"), "{msg}");
        assert_eq!(config_content(&home), "[memory]\nvector_enabled = false\n");
    });
}
