//! Issue #235: a TOML parse error in `tools.toml` (e.g. a duplicate key)
//! silently dropped EVERY dynamic tool and, worse, a subsequent mutating call
//! would overwrite the unparseable file and destroy it on disk.
//!
//! These tests pin the corrected contract:
//! - parse errors surface as `Err` / a logged failure, not an empty config;
//! - `load` degrades to zero tools (never crashes startup);
//! - `reload` returns `Err` and leaves the live registry untouched;
//! - mutating calls (`add_tool`) refuse to run, preserving the file bytes;
//! - on parse error, the loader tries a `.bak` last_good snapshot before giving up;
//! - every successful write snapshots the current parseable file as `.bak`.

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

// ---------------------------------------------------------------------------
// last_good backup tests (crash recovery, issue #235)
// ---------------------------------------------------------------------------

#[test]
fn write_creates_backup_before_overwrite() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("tools.toml");
    let backup = path.with_extension("toml.bak");
    let reg = Arc::new(ToolRegistry::new());

    // Initial write (no file exists yet, no backup needed).
    std::fs::write(&path, VALID_TOML).unwrap();
    assert!(!backup.exists(), "no backup when writing to a new file");

    // Adding a tool writes through write_config, which should snapshot
    // the current parseable file as .bak before overwriting.
    DynamicToolLoader::add_tool(&path, sample_def("gamma"), &reg).unwrap();
    assert!(backup.exists(), "backup must be created on write");

    let bak_content = std::fs::read_to_string(&backup).unwrap();
    assert!(
        bak_content.contains("alpha") && bak_content.contains("beta"),
        "backup must contain the original file content"
    );
    assert!(
        !bak_content.contains("gamma"),
        "backup must be the PRE-write snapshot, not the new content"
    );
}

#[test]
fn load_recovers_from_backup_on_corrupt_main() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("tools.toml");
    let reg = Arc::new(ToolRegistry::new());

    // Write valid file, then add a tool to create a .bak snapshot.
    std::fs::write(&path, VALID_TOML).unwrap();
    DynamicToolLoader::add_tool(&path, sample_def("gamma"), &reg).unwrap();

    // Corrupt the main file.
    std::fs::write(&path, DUPLICATE_KEY_TOML).unwrap();

    // Load should fall back to .bak and recover the original 2 tools.
    let reg2 = Arc::new(ToolRegistry::new());
    let count = DynamicToolLoader::load(&path, &reg2);
    assert_eq!(count, 2, "should recover 2 tools from backup, got {count}");
    assert!(reg2.has_tool("alpha"));
    assert!(reg2.has_tool("beta"));
}

#[test]
fn reload_recovers_from_backup_on_corrupt_main() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("tools.toml");
    let reg = Arc::new(ToolRegistry::new());

    std::fs::write(&path, VALID_TOML).unwrap();
    DynamicToolLoader::load(&path, &reg);
    DynamicToolLoader::add_tool(&path, sample_def("gamma"), &reg).unwrap();
    assert!(reg.has_tool("gamma"));

    // Corrupt the main file.
    std::fs::write(&path, DUPLICATE_KEY_TOML).unwrap();

    // Reload should recover from .bak, NOT return Err and NOT wipe the registry.
    let count = DynamicToolLoader::reload(&path, &reg).unwrap();
    assert_eq!(count, 2, "should recover 2 tools from backup");
    assert!(reg.has_tool("alpha"));
    assert!(reg.has_tool("beta"));
}

#[test]
fn add_tool_recovers_from_backup_and_repairs_file() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("tools.toml");
    let reg = Arc::new(ToolRegistry::new());

    // Write valid file, then add a tool to create a .bak.
    std::fs::write(&path, VALID_TOML).unwrap();
    DynamicToolLoader::add_tool(&path, sample_def("gamma"), &reg).unwrap();

    // Corrupt the main file.
    std::fs::write(&path, DUPLICATE_KEY_TOML).unwrap();

    // add_tool should recover from .bak, add the new tool, and repair the file.
    DynamicToolLoader::add_tool(&path, sample_def("delta"), &reg).unwrap();
    assert!(reg.has_tool("delta"));

    // File should now be valid again.
    let tools = DynamicToolLoader::list_tools_detailed(&path).unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(
        names.contains(&"delta"),
        "repaired file must contain the newly added tool"
    );
}

#[test]
fn corrupt_backup_and_corrupt_main_still_fails() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("tools.toml");
    let backup = path.with_extension("toml.bak");
    let reg = Arc::new(ToolRegistry::new());

    // Create a .bak via normal flow.
    std::fs::write(&path, VALID_TOML).unwrap();
    DynamicToolLoader::add_tool(&path, sample_def("gamma"), &reg).unwrap();
    assert!(backup.exists());

    // Corrupt both files.
    std::fs::write(&path, DUPLICATE_KEY_TOML).unwrap();
    std::fs::write(&backup, DUPLICATE_KEY_TOML).unwrap();

    // Load must still return 0 (no valid source).
    let reg2 = Arc::new(ToolRegistry::new());
    assert_eq!(
        DynamicToolLoader::load(&path, &reg2),
        0,
        "with both main and backup corrupt, load must degrade to 0 tools"
    );
}

#[test]
fn no_backup_for_already_corrupt_file() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("tools.toml");
    let backup = path.with_extension("toml.bak");
    let reg = Arc::new(ToolRegistry::new());

    // Write a corrupt file directly.
    std::fs::write(&path, DUPLICATE_KEY_TOML).unwrap();

    // add_tool fails (can't parse), so write_config is never reached.
    let result = DynamicToolLoader::add_tool(&path, sample_def("new"), &reg);
    assert!(result.is_err());
    assert!(
        !backup.exists(),
        "a corrupt file should never produce a backup"
    );
}
