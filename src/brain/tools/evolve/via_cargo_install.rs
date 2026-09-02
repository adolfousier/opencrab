//! The `cargo install opencrabs --force` upgrade strategy, for installs
//! that came from crates.io rather than a release binary.

use super::super::error::{Result, ToolError};
use super::super::r#trait::ToolResult;
use super::EvolveTool;
use crate::brain::agent::ProgressEvent;

impl EvolveTool {
    /// Update via `cargo install opencrabs --force`.
    pub(super) async fn evolve_via_cargo_install(
        &self,
        sid: uuid::Uuid,
        current_version: &str,
        latest_version: &str,
    ) -> Result<ToolResult> {
        if let Some(ref cb) = self.progress {
            cb(
                sid,
                ProgressEvent::IntermediateText {
                    text: format!(
                        "Updating via cargo install (v{} -> v{})...",
                        current_version, latest_version
                    ),
                    reasoning: None,
                },
            );
        }

        tracing::info!(
            target: "evolve",
            current_version,
            latest_version,
            session_id = %sid,
            "evolve: running `cargo install opencrabs --force`"
        );
        let output = tokio::process::Command::new("cargo")
            .args(["install", "opencrabs", "--force"])
            .stdin(std::process::Stdio::null())
            .output()
            .await
            .map_err(|e| {
                tracing::warn!(
                    target: "evolve",
                    error = %e,
                    session_id = %sid,
                    "evolve: failed to spawn `cargo` — is the Rust toolchain installed?"
                );
                ToolError::Execution(format!("Failed to spawn cargo: {e}"))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stderr_excerpt: String = stderr.chars().take(500).collect();
            tracing::warn!(
                target: "evolve",
                exit_status = %output.status,
                stderr_excerpt = %stderr_excerpt,
                session_id = %sid,
                "evolve: cargo install failed"
            );
            return Ok(ToolResult::error(format!(
                "cargo install failed: {stderr_excerpt}"
            )));
        }

        // Signal restart
        if let Some(ref cb) = self.progress {
            cb(
                sid,
                ProgressEvent::RestartReady {
                    status: format!(
                        "Evolved via cargo install: v{} -> v{}.",
                        current_version, latest_version
                    ),
                    // evolve replaced the running exe in place; the handler
                    // resolves it via current_exe().
                    binary_path: None,
                },
            );
        }

        Ok(ToolResult::success(format!(
            "Evolved from v{} to v{} via cargo install.",
            current_version, latest_version
        )))
    }
}
