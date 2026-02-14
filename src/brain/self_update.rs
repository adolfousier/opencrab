//! Self-Update Module
//!
//! Handles building, testing, and hot-restarting OpenCrabs.
//! The running binary is in memory — modifying source on disk is safe.
//! After a successful build, `exec()` replaces the current process with the new binary.

use anyhow::Result;
use std::path::PathBuf;
use uuid::Uuid;

/// Handles building, testing, and restarting OpenCrabs from source.
pub struct SelfUpdater {
    /// Root of the OpenCrabs project (where Cargo.toml lives)
    project_root: PathBuf,
    /// Path to the compiled binary
    binary_path: PathBuf,
}

impl SelfUpdater {
    /// Create a new SelfUpdater.
    ///
    /// `project_root` — directory containing Cargo.toml
    /// `binary_path` — where the release binary will be after build
    pub fn new(project_root: PathBuf, binary_path: PathBuf) -> Self {
        Self {
            project_root,
            binary_path,
        }
    }

    /// Auto-detect project root and binary path from the current executable.
    pub fn auto_detect() -> Result<Self> {
        let exe = std::env::current_exe()?;

        // Walk up from the executable to find Cargo.toml
        let mut project_root = exe
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Cannot determine executable parent directory"))?
            .to_path_buf();

        // Try to find Cargo.toml by walking up directories
        loop {
            if project_root.join("Cargo.toml").exists() {
                break;
            }
            if !project_root.pop() {
                return Err(anyhow::anyhow!(
                    "Could not find Cargo.toml in any parent directory of {}",
                    exe.display()
                ));
            }
        }

        let binary_path = project_root
            .join("target")
            .join("release")
            .join("opencrabs");

        Ok(Self {
            project_root,
            binary_path,
        })
    }

    /// Build the project with `cargo build --release`.
    ///
    /// Returns `Ok(binary_path)` on success or `Err(compiler_output)` on failure.
    pub async fn build(&self) -> Result<PathBuf, String> {
        tracing::info!("Building OpenCrabs at {}", self.project_root.display());

        let output = tokio::process::Command::new("cargo")
            .arg("build")
            .arg("--release")
            .current_dir(&self.project_root)
            .output()
            .await
            .map_err(|e| format!("Failed to spawn cargo build: {}", e))?;

        if output.status.success() {
            tracing::info!("Build succeeded: {}", self.binary_path.display());
            Ok(self.binary_path.clone())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            tracing::error!("Build failed:\n{}\n{}", stderr, stdout);
            Err(format!("{}\n{}", stderr, stdout))
        }
    }

    /// Run tests with `cargo test`.
    ///
    /// Returns `Ok(())` on success or `Err(test_output)` on failure.
    pub async fn test(&self) -> Result<(), String> {
        tracing::info!("Running tests at {}", self.project_root.display());

        let output = tokio::process::Command::new("cargo")
            .arg("test")
            .current_dir(&self.project_root)
            .output()
            .await
            .map_err(|e| format!("Failed to spawn cargo test: {}", e))?;

        if output.status.success() {
            tracing::info!("Tests passed");
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            tracing::warn!("Tests failed:\n{}\n{}", stderr, stdout);
            Err(format!("{}\n{}", stderr, stdout))
        }
    }

    /// Replace the running process with the new binary via Unix exec().
    ///
    /// Passes `chat --session <session_id>` to resume the same session.
    /// This function only returns on error — on success, the process is replaced.
    #[cfg(unix)]
    pub fn restart(&self, session_id: Uuid) -> Result<()> {
        use std::os::unix::process::CommandExt;

        tracing::info!(
            "Restarting OpenCrabs: {} chat --session {}",
            self.binary_path.display(),
            session_id
        );

        let err = std::process::Command::new(&self.binary_path)
            .args(["chat", "--session", &session_id.to_string()])
            .exec(); // Replaces the process — only returns on error

        Err(anyhow::anyhow!("exec() failed: {}", err))
    }

    /// On non-Unix platforms, restart is not supported via exec().
    #[cfg(not(unix))]
    pub fn restart(&self, _session_id: Uuid) -> Result<()> {
        Err(anyhow::anyhow!(
            "Hot restart via exec() is only supported on Unix platforms"
        ))
    }

    /// Get the project root path.
    pub fn project_root(&self) -> &std::path::Path {
        &self.project_root
    }

    /// Get the binary path.
    pub fn binary_path(&self) -> &std::path::Path {
        &self.binary_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let updater = SelfUpdater::new(
            PathBuf::from("/tmp/project"),
            PathBuf::from("/tmp/project/target/release/opencrabs"),
        );
        assert_eq!(
            updater.project_root(),
            std::path::Path::new("/tmp/project")
        );
        assert_eq!(
            updater.binary_path(),
            std::path::Path::new("/tmp/project/target/release/opencrabs")
        );
    }
}
