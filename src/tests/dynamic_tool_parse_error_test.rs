//! Issue #235: a TOML parse error in `tools.toml` (e.g. a duplicate key)
//! silently dropped EVERY dynamic tool and, worse, a subsequent mutating call
//! would overwrite the unparseable file and destroy it on disk.
//!
//! These tests pin the corrected contract:
//! - parse errors surface as `Err` / a logged failure, not an empty config;
//! - `load` degrades to zero tools (never crashes startup);
//! - `reload` returns `Err` and leaves the live registry untouched;
//! - mutating calls (`add_tool`) refuse to run, preserving the file bytes.

use crate::brain::tools::ToolRegistry;
use crate::brain::tools::dynamic::{DynamicTool, DynamicToolDef, DynamicToolLoader, ExecutorType};
use std::collections::HashMap;
use std::sync::Arc;
use tempfile::TempDir;

/// A `tools.toml` with TWO valid tools plus a duplicate `name` key in the
/// second table — TOML rejects the whole document, so all tools "vanish".
const DUPLICATE_KEY_TOML: &str = "\
[[tools]]
name = \"alpha\"
description = \"first\"
executor = \"shell\"
command = \"echo a\"

[[tools]]
name = \"beta\"
description = \"second\"
executor = \"shell\"
command = \"echo b\"
name = \"beta_dup\"
";

const VALID_TOML: &str = "\
[[tools]]
name = \"alpha\"
description = \"first\"
executor = \"shell\"
command = \"echo a\"

[[tools]]
name = \"beta\"
description = \"second\"
executor = \"shell\"
command = \"echo b\"
";

fn write_tools(contents: &str) -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("tools.toml");
    std::fs::write(&path, contents).unwrap();
    (dir, path)
}

fn sample_def(name: &str) -> DynamicToolDef {
    DynamicToolDef {
        name: name.into(),
        description: "added".into(),
        executor: ExecutorType::Shell,
        enabled: true,
        requires_approval: false,
        method: None,
        url: None,
        headers: HashMap::new(),
        timeout_secs: 10,
        command: Some("echo".into()),
        params: vec![],
    }
}

#[test]
fn parse_error_load_registers_nothing() {
    let (_dir, path) = write_tools(DUPLICATE_KEY_TOML);
    let reg = Arc::new(ToolRegistry::new());
    // Previously this returned 0 by silently swallowing the error; now it
    // still returns 0 but read_config has logged it at ERROR.
    assert_eq!(DynamicToolLoader::load(&path, &reg), 0);
    assert!(!reg.has_tool("alpha"));
}

#[test]
fn parse_error_list_is_err_not_empty() {
    let (_dir, path) = write_tools(DUPLICATE_KEY_TOML);
    assert!(
        DynamicToolLoader::list_tools_detailed(&path).is_err(),
        "a parse error must surface as Err, not an empty tool list"
    );
}

#[test]
fn parse_error_reload_is_err_and_keeps_live_registry() {
    let (_dir, path) = write_tools(DUPLICATE_KEY_TOML);
    let reg = Arc::new(ToolRegistry::new());

    // Pretend a previous good load already registered a tool.
    reg.register(Arc::new(DynamicTool::new(sample_def("already_live"))));
    assert!(reg.has_tool("already_live"));

    // Reloading a broken file must fail loudly and NOT strip the live tool.
    assert!(
        DynamicToolLoader::reload(&path, &reg).is_err(),
        "reload of an unparseable file must return Err"
    );
    assert!(
        reg.has_tool("already_live"),
        "a broken reload must leave already-loaded tools untouched"
    );
}

#[test]
fn add_tool_on_unparseable_file_does_not_destroy_it() {
    let (_dir, path) = write_tools(DUPLICATE_KEY_TOML);
    let reg = Arc::new(ToolRegistry::new());
    let before = std::fs::read_to_string(&path).unwrap();

    let result = DynamicToolLoader::add_tool(&path, sample_def("gamma"), &reg);
    assert!(
        result.is_err(),
        "add_tool must refuse to write over an unparseable file"
    );

    let after = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        before, after,
        "the original tools.toml must be preserved byte-for-byte, not overwritten empty"
    );
    assert!(!reg.has_tool("gamma"));
}

#[test]
fn valid_file_still_loads_all_tools() {
    let (_dir, path) = write_tools(VALID_TOML);
    let reg = Arc::new(ToolRegistry::new());
    assert_eq!(DynamicToolLoader::load(&path, &reg), 2);
    assert!(reg.has_tool("alpha"));
    assert!(reg.has_tool("beta"));
}

#[test]
fn missing_file_is_not_an_error() {
    let reg = Arc::new(ToolRegistry::new());
    let missing = std::path::Path::new("/nonexistent/tools.toml");
    assert_eq!(DynamicToolLoader::load(missing, &reg), 0);
    assert!(
        DynamicToolLoader::list_tools_detailed(missing)
            .unwrap()
            .is_empty(),
        "a missing tools.toml is a fresh setup, not a parse error"
    );
}
