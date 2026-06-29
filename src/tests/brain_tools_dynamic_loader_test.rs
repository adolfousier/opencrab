use crate::brain::tools::ToolRegistry;
use crate::brain::tools::dynamic::loader::*;
use crate::brain::tools::dynamic::tool::DynamicToolDef;
use crate::brain::tools::dynamic::tool::{ExecutorType, ParamDef};
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;

fn tmp_path() -> (TempDir, PathBuf) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("tools.toml");
    (dir, path)
}

#[test]
fn test_load_nonexistent() {
    let reg = Arc::new(ToolRegistry::new());
    assert_eq!(DynamicToolLoader::load(Path::new("/nonexistent"), &reg), 0);
}

#[test]
fn test_add_and_list() {
    let (_dir, path) = tmp_path();
    let reg = Arc::new(ToolRegistry::new());
    let def = DynamicToolDef {
        name: "ping".into(),
        description: "Ping".into(),
        executor: ExecutorType::Shell,
        enabled: true,
        requires_approval: false,
        method: None,
        url: None,
        headers: HashMap::new(),
        timeout_secs: 10,
        command: Some("ping -c 1 {{host}}".into()),
        params: vec![ParamDef {
            name: "host".into(),
            param_type: "string".into(),
            description: "".into(),
            required: true,
            default: None,
            coerce_empty_to: Default::default(),
            coerce_null_to: Default::default(),
        }],
    };
    DynamicToolLoader::add_tool(&path, def, &reg).unwrap();
    assert!(reg.has_tool("ping"));
    assert_eq!(
        DynamicToolLoader::list_tools_detailed(&path).unwrap().len(),
        1
    );
}

#[test]
fn test_remove() {
    let (_dir, path) = tmp_path();
    let reg = Arc::new(ToolRegistry::new());
    let def = DynamicToolDef {
        name: "rm_me".into(),
        description: "".into(),
        executor: ExecutorType::Shell,
        enabled: true,
        requires_approval: false,
        method: None,
        url: None,
        headers: HashMap::new(),
        timeout_secs: 10,
        command: Some("echo".into()),
        params: vec![],
    };
    DynamicToolLoader::add_tool(&path, def, &reg).unwrap();
    assert!(DynamicToolLoader::remove_tool(&path, "rm_me", &reg).unwrap());
    assert!(!reg.has_tool("rm_me"));
}

#[test]
fn test_enable_disable() {
    let (_dir, path) = tmp_path();
    let reg = Arc::new(ToolRegistry::new());
    let def = DynamicToolDef {
        name: "tog".into(),
        description: "".into(),
        executor: ExecutorType::Shell,
        enabled: true,
        requires_approval: false,
        method: None,
        url: None,
        headers: HashMap::new(),
        timeout_secs: 10,
        command: Some("echo".into()),
        params: vec![],
    };
    DynamicToolLoader::add_tool(&path, def, &reg).unwrap();
    DynamicToolLoader::set_enabled(&path, "tog", false, &reg).unwrap();
    assert!(!reg.has_tool("tog"));
    DynamicToolLoader::set_enabled(&path, "tog", true, &reg).unwrap();
    assert!(reg.has_tool("tog"));
}

#[test]
fn test_reload() {
    let (_dir, path) = tmp_path();
    let reg = Arc::new(ToolRegistry::new());
    std::fs::write(&path, "[[tools]]\nname = \"disk\"\ndescription = \"From disk\"\nexecutor = \"shell\"\ncommand = \"echo\"").unwrap();
    assert_eq!(DynamicToolLoader::reload(&path, &reg).unwrap(), 1);
    assert!(reg.has_tool("disk"));
}

#[test]
fn test_disabled_not_registered() {
    let (_dir, path) = tmp_path();
    let reg = Arc::new(ToolRegistry::new());
    std::fs::write(&path, "[[tools]]\nname = \"on\"\ndescription = \"\"\nexecutor = \"shell\"\ncommand = \"echo\"\n\n[[tools]]\nname = \"off\"\ndescription = \"\"\nexecutor = \"shell\"\ncommand = \"echo\"\nenabled = false").unwrap();
    assert_eq!(DynamicToolLoader::load(&path, &reg), 1);
    assert!(reg.has_tool("on"));
    assert!(!reg.has_tool("off"));
}
