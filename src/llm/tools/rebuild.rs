//! Rebuild Tool
//!
//! Lets the agent build OpenCrabs from source and signal the TUI to restart.
//! The build runs via `SelfUpdater`; on success a `ProgressEvent::RestartReady`
//! is emitted so the TUI can offer `exec()` hot-restart to the user.

use super::error::Result;
use super::r#trait::{Tool, ToolCapability, ToolExecutionContext, ToolResult};
use crate::brain::SelfUpdater;
use crate::llm::agent::{ProgressCallback, ProgressEvent};
use async_trait::async_trait;
use serde_json::Value;

/// Agent-callable tool that builds the project and signals restart readiness.
pub struct RebuildTool {
    progress: Option<ProgressCallback>,
}

impl RebuildTool {
    pub fn new(progress: Option<ProgressCallback>) -> Self {
        Self { progress }
    }
}

#[async_trait]
impl Tool for RebuildTool {
    fn name(&self) -> &str {
        "rebuild"
    }

    fn description(&self) -> &str {
        "Build OpenCrabs from source (cargo build --release) and signal the TUI to hot-restart. \
         Call this after editing source code to apply your changes. \
         On success the user will be prompted to restart; on failure the compiler output is returned."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::SystemModification]
    }

    async fn execute(&self, _input: Value, _context: &ToolExecutionContext) -> Result<ToolResult> {
        let updater = match SelfUpdater::auto_detect() {
            Ok(u) => u,
            Err(e) => {
                return Ok(ToolResult::error(format!(
                    "Cannot detect project root: {}",
                    e
                )));
            }
        };

        match updater.build().await {
            Ok(path) => {
                if let Some(ref cb) = self.progress {
                    cb(ProgressEvent::RestartReady {
                        status: "Build successful".into(),
                    });
                }
                Ok(ToolResult::success(format!(
                    "Build successful: {}. The user has been prompted to restart.",
                    path.display()
                )))
            }
            Err(output) => Ok(ToolResult::error(format!("Build failed:\n{}", output))),
        }
    }
}
