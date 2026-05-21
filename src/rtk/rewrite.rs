//! RTK command rewriting functionality
//!
//! RTK works as a direct proxy: you run `rtk git status` instead of `git status`.
//! RTK intercepts the command, runs it internally, filters/compresses the output,
//! and returns the token-optimized version.
//!
//! This module maintains the list of supported commands and handles the rewriting.

use std::process::Command;
use std::sync::OnceLock;

/// Commands that RTK supports as a proxy (from `rtk --help`).
/// When the first token of a bash command matches one of these, we prepend `rtk`.
///
/// Meta-commands (gain, config, init, session, telemetry, learn, discover) are
/// excluded because they're RTK's own management commands, not proxies.
const RTK_SUPPORTED_COMMANDS: &[&str] = &[
    "git",
    "gh",
    "glab",
    "aws",
    "psql",
    "pnpm",
    "npm",
    "npx",
    "cargo",
    "docker",
    "kubectl",
    "grep",
    "find",
    "ls",
    "tree",
    "diff",
    "curl",
    "wget",
    "jest",
    "vitest",
    "prisma",
    "tsc",
    "next",
    "dotnet",
    "playwright",
    "prettier",
    "eslint",
];

/// Commands that should NEVER be rewritten even if they match the list above.
/// These are either too interactive, have side effects we don't want RTK to
/// intercept, or are already RTK meta-commands.
const RTK_BLOCKLIST: &[&str] = &[
    "rtk",       // Don't double-prepend
    "sudo",      // Don't rewrite elevated commands
    "ssh",       // Interactive, needs TTY
    "scp",       // Binary transfer
    "sftp",      // Interactive
    "rsync",     // Binary transfer
    "vim",       // Editor
    "vi",        // Editor
    "nvim",      // Editor
    "nano",      // Editor
    "emacs",     // Editor
    "less",      // Pager
    "more",      // Pager
    "man",       // Pager
    "python",    // REPL
    "python3",   // REPL
    "node",      // REPL
    "mysql",     // REPL
    "redis-cli", // REPL
    "psql",      // REPL (even though rtk supports it, we handle it via tool)
];

/// Result of RTK command rewriting
#[derive(Debug, Clone)]
pub struct RtkResult {
    /// The rewritten command (with rtk prefix)
    pub rewritten_command: String,
    /// Whether the command was actually rewritten
    pub was_rewritten: bool,
    /// Original command for reference
    pub original_command: String,
}

/// Check if the rtk binary is available in PATH.
///
/// Result is cached after the first call.
pub fn is_rtk_available() -> bool {
    static RTK_AVAILABLE: OnceLock<bool> = OnceLock::new();

    *RTK_AVAILABLE.get_or_init(|| match Command::new("which").arg("rtk").output() {
        Ok(output) => {
            let available = output.status.success();
            if available {
                tracing::info!("RTK binary found in PATH");
            } else {
                tracing::info!("RTK binary not found in PATH");
            }
            available
        }
        Err(_) => {
            tracing::warn!("Failed to check for rtk binary availability");
            false
        }
    })
}

/// Extract the first real command token from a shell command string.
///
/// Skips leading env var assignments (`FOO=bar cmd`) and returns the
/// actual command name.
fn first_command_token(command: &str) -> &str {
    for token in command.split_whitespace() {
        // Skip env var assignments like FOO=bar
        if token.contains('=') && !token.starts_with('-') && !token.starts_with('/') {
            continue;
        }
        return token;
    }
    ""
}

/// Check if a command token is supported by RTK for rewriting.
fn is_rtk_supported(token: &str) -> bool {
    // Strip leading path: /usr/bin/git → git
    let basename = token.rsplit('/').next().unwrap_or(token);

    if RTK_BLOCKLIST.contains(&basename) {
        return false;
    }

    RTK_SUPPORTED_COMMANDS.contains(&basename)
}

/// Rewrite a bash command to use RTK as a proxy.
///
/// If the command's first token is RTK-supported (git, cargo, npm, etc.),
/// prepends `rtk` to the command. Otherwise returns None.
///
/// # Example
/// ```rust
/// use opencrabs::rtk::rewrite_command;
///
/// let result = rewrite_command("git status");
/// // Returns Some(RtkResult { rewritten_command: "rtk git status", ... })
///
/// let result = rewrite_command("echo hello");
/// // Returns None (echo is not RTK-supported)
/// ```
pub fn rewrite_command(command: &str) -> Option<RtkResult> {
    if !is_rtk_available() {
        tracing::debug!("RTK not available, skipping command rewrite");
        return None;
    }

    let trimmed = command.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Don't rewrite commands that already start with rtk
    if trimmed.starts_with("rtk ") || trimmed == "rtk" {
        return None;
    }

    // Handle chained commands: only rewrite if the FIRST command is supported.
    // For `git status && echo done`, we rewrite to `rtk git status && echo done`.
    // For `echo start && git status`, we don't rewrite (first cmd not supported).
    let first_token = first_command_token(trimmed);

    if !is_rtk_supported(first_token) {
        tracing::debug!(
            "RTK: command '{}' not supported (token: '{}')",
            command,
            first_token
        );
        return None;
    }

    let rewritten = format!("rtk {}", trimmed);

    tracing::debug!("RTK rewrote: '{}' -> '{}'", command, rewritten);

    Some(RtkResult {
        rewritten_command: rewritten,
        was_rewritten: true,
        original_command: command.to_string(),
    })
}

/// Convenience wrapper: returns just the rewritten string or None.
#[allow(dead_code)]
pub fn rewrite_command_string(command: &str) -> Option<String> {
    rewrite_command(command).map(|r| r.rewritten_command)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_first_command_token_simple() {
        assert_eq!(first_command_token("git status"), "git");
        assert_eq!(first_command_token("cargo build"), "cargo");
        assert_eq!(first_command_token("echo hello"), "echo");
    }

    #[test]
    fn test_first_command_token_with_env() {
        assert_eq!(first_command_token("FOO=bar git status"), "git");
        assert_eq!(first_command_token("PATH=/usr/bin cargo test"), "cargo");
    }

    #[test]
    fn test_rtk_supported() {
        assert!(is_rtk_supported("git"));
        assert!(is_rtk_supported("cargo"));
        assert!(is_rtk_supported("npm"));
        assert!(is_rtk_supported("docker"));
        assert!(!is_rtk_supported("echo"));
        assert!(!is_rtk_supported("cat"));
        assert!(!is_rtk_supported("rm"));
    }

    #[test]
    fn test_rtk_blocklist() {
        assert!(!is_rtk_supported("sudo"));
        assert!(!is_rtk_supported("ssh"));
        assert!(!is_rtk_supported("vim"));
        assert!(!is_rtk_supported("rtk"));
    }

    #[test]
    fn test_rtk_supported_with_path() {
        assert!(is_rtk_supported("/usr/bin/git"));
        assert!(is_rtk_supported("/usr/local/bin/cargo"));
    }

    #[test]
    fn test_rewrite_git_status() {
        // This test depends on rtk being installed
        if !is_rtk_available() {
            return;
        }
        let result = rewrite_command("git status");
        assert!(result.is_some());
        let r = result.unwrap();
        assert!(r.was_rewritten);
        assert_eq!(r.rewritten_command, "rtk git status");
    }

    #[test]
    fn test_rewrite_echo_not_supported() {
        let result = rewrite_command("echo hello");
        assert!(result.is_none());
    }

    #[test]
    fn test_rewrite_already_rtk() {
        let result = rewrite_command("rtk git status");
        assert!(result.is_none());
    }

    #[test]
    fn test_rewrite_sudo_blocked() {
        let result = rewrite_command("sudo git pull");
        assert!(result.is_none());
    }

    #[test]
    fn test_rewrite_chained_command() {
        if !is_rtk_available() {
            return;
        }
        let result = rewrite_command("git status && echo done");
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.rewritten_command, "rtk git status && echo done");
    }

    #[test]
    fn test_rewrite_empty_command() {
        let result = rewrite_command("");
        assert!(result.is_none());
    }

    #[test]
    fn test_rewrite_cargo_test() {
        if !is_rtk_available() {
            return;
        }
        let result = rewrite_command("cargo test --release");
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.rewritten_command, "rtk cargo test --release");
    }

    #[test]
    fn test_rewrite_npm_install() {
        if !is_rtk_available() {
            return;
        }
        let result = rewrite_command("npm install express");
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.rewritten_command, "rtk npm install express");
    }

    #[test]
    fn test_rewrite_env_prefix() {
        if !is_rtk_available() {
            return;
        }
        let result = rewrite_command("RUST_LOG=debug cargo build");
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.rewritten_command, "rtk RUST_LOG=debug cargo build");
    }
}
