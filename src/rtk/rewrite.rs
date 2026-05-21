//! RTK command rewriting functionality
//!
//! This module provides functions to check if RTK is available and to rewrite
//! bash commands to use RTK for token savings.

use std::process::Command;
use std::sync::OnceLock;

/// Result of RTK command rewriting
#[derive(Debug, Clone)]
pub struct RtkResult {
    /// The rewritten command (with rtk prefix)
    pub rewritten_command: String,
    /// Whether the command was actually rewritten (false if rtk doesn't support it)
    pub was_rewritten: bool,
    /// Original command for reference
    pub original_command: String,
}

/// Check if the rtk binary is available in PATH
///
/// Uses `which` command to check for rtk availability. The result is cached
/// after the first call to avoid repeated subprocess overhead.
///
/// # Returns
/// - `true` if rtk is available
/// - `false` if rtk is not installed or not in PATH
pub fn is_rtk_available() -> bool {
    static RTK_AVAILABLE: OnceLock<bool> = OnceLock::new();

    *RTK_AVAILABLE.get_or_init(|| {
        // Use 'which' to check if rtk is in PATH
        match Command::new("which").arg("rtk").output() {
            Ok(output) => output.status.success(),
            Err(_) => {
                tracing::warn!("Failed to check for rtk binary availability");
                false
            }
        }
    })
}

/// Rewrite a bash command to use RTK for token savings
///
/// Calls `rtk rewrite <command>` to get the rewritten version. If RTK doesn't
/// support the command or isn't available, returns None.
///
/// # Arguments
/// * `command` - The original bash command to rewrite
///
/// # Returns
/// - `Some(RtkResult)` if the command was rewritten
/// - `None` if RTK is not available or doesn't support this command
///
/// # Example
/// ```rust
/// use opencrabs::rtk::rewrite_command;
///
/// if let Some(result) = rewrite_command("git status") {
///     println!("Rewritten: {}", result.rewritten_command);
///     // Output: "rtk git status"
/// }
/// ```
pub fn rewrite_command(command: &str) -> Option<RtkResult> {
    if !is_rtk_available() {
        tracing::debug!("RTK not available, skipping command rewrite");
        return None;
    }

    // Call rtk rewrite to get the rewritten command
    let output = Command::new("rtk")
        .arg("rewrite")
        .arg(command)
        .output()
        .ok()?;

    if !output.status.success() {
        tracing::debug!("RTK rewrite failed for command: {}", command);
        return None;
    }

    let rewritten = String::from_utf8_lossy(&output.stdout).trim().to_string();

    // If RTK returns empty or the same command, it doesn't support this command
    if rewritten.is_empty() || rewritten == command {
        tracing::debug!("RTK doesn't support command: {}", command);
        return None;
    }

    tracing::debug!("RTK rewrote command: '{}' -> '{}'", command, rewritten);

    Some(RtkResult {
        rewritten_command: rewritten,
        was_rewritten: true,
        original_command: command.to_string(),
    })
}

/// Rewrite a command and return just the rewritten string, or None if not supported
///
/// This is a convenience wrapper around `rewrite_command` that returns only
/// the rewritten command string.
///
/// # Arguments
/// * `command` - The original bash command
///
/// # Returns
/// - `Some(String)` with the rewritten command
/// - `None` if not rewritten
#[allow(dead_code)]
pub fn rewrite_command_string(command: &str) -> Option<String> {
    rewrite_command(command).map(|r| r.rewritten_command)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rtk_availability_check() {
        // This test will pass or fail depending on whether rtk is installed
        // We just verify it doesn't panic
        let _ = is_rtk_available();
    }

    #[test]
    fn test_rewrite_unsupported_command() {
        // Test with a command RTK likely doesn't support
        let result = rewrite_command("echo hello");
        // Should return None since echo is not in RTK's supported commands
        assert!(result.is_none() || !result.unwrap().was_rewritten);
    }

    #[test]
    fn test_rewrite_git_status() {
        // Test with a command RTK should support
        let result = rewrite_command("git status");
        // If RTK is installed, this should be rewritten
        if is_rtk_available() {
            assert!(result.is_some());
            let r = result.unwrap();
            assert!(r.was_rewritten);
            assert!(r.rewritten_command.starts_with("rtk"));
        }
    }
}
