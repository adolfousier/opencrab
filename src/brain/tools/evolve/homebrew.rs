//! Upgrading a Homebrew-managed install.
//!
//! Homebrew owns the Cellar binary and records which version it believes is
//! installed. Renaming a downloaded binary over it SUCCEEDS, because the prefix
//! is user-owned on Apple Silicon, and then brew's manifest disagrees with the
//! disk until an unrelated `brew upgrade` silently reverts the user. Letting
//! brew do the upgrade keeps the version it reports true (#963).

use crate::brain::agent::{ProgressCallback, ProgressEvent};
use crate::brain::tools::error::{Result, ToolError};
use crate::brain::tools::r#trait::ToolResult;

/// The formula name, which is also the binary name.
const FORMULA: &str = "opencrabs";

/// Why an upgrade attempt did not happen, phrased for the user.
///
/// Pure so the wording is testable without a Homebrew install: a refusal the
/// user cannot act on is worse than the silent overwrite it replaced.
pub(crate) fn spawn_failure_message(err: &str) -> String {
    format!(
        "This build is managed by Homebrew, but `brew` could not be run: {err}. \
         Upgrade with `brew upgrade {FORMULA}`."
    )
}

/// Message for a `brew upgrade` that ran and failed.
pub(crate) fn upgrade_failure_message(stderr: &str) -> String {
    let excerpt: String = stderr.chars().take(500).collect();
    format!("brew upgrade failed: {excerpt}")
}

/// Run `brew upgrade opencrabs` and signal a restart.
pub(crate) async fn evolve(
    sid: uuid::Uuid,
    current_version: &str,
    latest_version: &str,
    progress: Option<&ProgressCallback>,
) -> Result<ToolResult> {
    tracing::info!(
        target: "evolve",
        current_version,
        latest_version,
        session_id = %sid,
        "evolve: running `brew upgrade opencrabs`"
    );

    // `brew update` first: upgrade resolves against the LOCAL formula index, so
    // without it brew can believe the installed version is already latest and
    // exit successfully having done nothing.
    match tokio::process::Command::new("brew")
        .arg("update")
        .stdin(std::process::Stdio::null())
        .output()
        .await
    {
        Ok(_) => {}
        Err(e) => tracing::warn!(
            target: "evolve",
            error = %e,
            session_id = %sid,
            "evolve: `brew update` failed, continuing on the existing formula index"
        ),
    }

    let output = tokio::process::Command::new("brew")
        .args(["upgrade", FORMULA])
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .map_err(|e| {
            tracing::warn!(
                target: "evolve",
                error = %e,
                session_id = %sid,
                "evolve: failed to spawn `brew` — is Homebrew on PATH?"
            );
            ToolError::Execution(spawn_failure_message(&e.to_string()))
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::warn!(
            target: "evolve",
            exit_status = %output.status,
            stderr_excerpt = %stderr.chars().take(500).collect::<String>(),
            session_id = %sid,
            "evolve: brew upgrade failed"
        );
        return Ok(ToolResult::error(upgrade_failure_message(&stderr)));
    }

    if let Some(cb) = progress {
        cb(
            sid,
            ProgressEvent::RestartReady {
                status: format!("Evolved via Homebrew: v{current_version} -> v{latest_version}."),
                // brew replaced the Cellar binary and repointed its symlink, so
                // the handler resolves the new one through current_exe().
                binary_path: None,
            },
        );
    }
    Ok(ToolResult::success(format!(
        "Upgraded via Homebrew: v{current_version} -> v{latest_version}. Restarting."
    )))
}
