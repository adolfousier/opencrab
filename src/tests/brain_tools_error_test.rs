use crate::brain::tools::ToolError;
use crate::brain::tools::error::*;

#[test]
fn test_tool_error_display() {
    let err = ToolError::NotFound("test_tool".to_string());
    assert_eq!(err.to_string(), "Tool not found: test_tool");

    let err = ToolError::PermissionDenied("dangerous_operation".to_string());
    assert_eq!(err.to_string(), "Permission denied: dangerous_operation");
}

#[test]
fn test_is_pre_execution_miss() {
    // The tool never ran: name didn't resolve, or args failed validation.
    assert!(ToolError::NotFound("x".into()).is_pre_execution_miss());
    assert!(ToolError::InvalidInput("x".into()).is_pre_execution_miss());
    // The tool ran (or tried) and genuinely errored: real reliability signal.
    assert!(!ToolError::Execution("x".into()).is_pre_execution_miss());
    assert!(!ToolError::PermissionDenied("x".into()).is_pre_execution_miss());
    assert!(!ToolError::ApprovalRequired("x".into()).is_pre_execution_miss());
    assert!(!ToolError::Timeout(5).is_pre_execution_miss());
}

#[test]
fn test_expand_tilde_prefix() {
    let home = dirs::home_dir().expect("home dir required for this test");
    assert_eq!(expand_tilde("~/foo/bar"), home.join("foo/bar"));
    assert_eq!(expand_tilde("~"), home);
}

#[test]
fn test_expand_tilde_passthrough() {
    // Tilde in the middle of a path is NOT a home reference — leave it alone.
    assert_eq!(
        expand_tilde("/tmp/~backup").to_string_lossy(),
        "/tmp/~backup"
    );
    assert_eq!(expand_tilde("foo/bar").to_string_lossy(), "foo/bar");
    assert_eq!(expand_tilde("/abs/path").to_string_lossy(), "/abs/path");
}

#[test]
fn test_resolve_tool_path_tilde_becomes_absolute() {
    // The classic custom-provider bug: model sends `~/.opencrabs/logs`,
    // cwd is `/Users/adolfo/srv/rs/opencrabs`. Before the fix this
    // produced `/Users/adolfo/srv/rs/opencrabs/~/.opencrabs/logs`.
    let cwd = std::path::Path::new("/Users/adolfo/srv/rs/opencrabs");
    let resolved = resolve_tool_path("~/.opencrabs/logs", cwd);
    let home = dirs::home_dir().expect("home dir required");
    assert_eq!(resolved, home.join(".opencrabs/logs"));
    assert!(resolved.is_absolute());
}

#[test]
fn test_resolve_tool_path_relative_joins_cwd() {
    let cwd = std::path::Path::new("/tmp/project");
    assert_eq!(
        resolve_tool_path("src/main.rs", cwd),
        std::path::PathBuf::from("/tmp/project/src/main.rs"),
    );
}

#[test]
fn test_resolve_tool_path_absolute_passthrough() {
    let cwd = std::path::Path::new("/tmp/project");
    assert_eq!(
        resolve_tool_path("/etc/hosts", cwd),
        std::path::PathBuf::from("/etc/hosts"),
    );
}
