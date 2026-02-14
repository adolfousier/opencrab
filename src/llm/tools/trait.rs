//! Tool trait definition

use super::error::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use uuid::Uuid;

/// Execution context for tools
#[derive(Debug, Clone)]
pub struct ToolExecutionContext {
    /// Session ID
    pub session_id: Uuid,

    /// Working directory
    pub working_directory: std::path::PathBuf,

    /// Environment variables
    pub env_vars: HashMap<String, String>,

    /// Whether auto-approve is enabled
    pub auto_approve: bool,

    /// Maximum execution timeout in seconds
    pub timeout_secs: u64,

    /// Whether in read-only mode (Plan mode) - restricts write operations
    pub read_only_mode: bool,
}

impl ToolExecutionContext {
    /// Create a new execution context
    pub fn new(session_id: Uuid) -> Self {
        Self {
            session_id,
            working_directory: std::env::current_dir().unwrap_or_default(),
            env_vars: HashMap::new(),
            auto_approve: false,
            timeout_secs: 30,
            read_only_mode: false,
        }
    }

    /// Set working directory
    pub fn with_working_directory(mut self, dir: std::path::PathBuf) -> Self {
        self.working_directory = dir;
        self
    }

    /// Set auto-approve
    pub fn with_auto_approve(mut self, auto_approve: bool) -> Self {
        self.auto_approve = auto_approve;
        self
    }

    /// Set timeout
    pub fn with_timeout(mut self, timeout_secs: u64) -> Self {
        self.timeout_secs = timeout_secs;
        self
    }

    /// Set read-only mode (for Plan mode)
    pub fn with_read_only_mode(mut self, read_only: bool) -> Self {
        self.read_only_mode = read_only;
        self
    }
}

/// Tool result
#[derive(Debug, Clone)]
pub struct ToolResult {
    /// Whether the execution was successful
    pub success: bool,

    /// Output from the tool
    pub output: String,

    /// Error message if unsuccessful
    pub error: Option<String>,

    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

impl ToolResult {
    /// Create a successful result
    pub fn success(output: String) -> Self {
        Self {
            success: true,
            output,
            error: None,
            metadata: HashMap::new(),
        }
    }

    /// Create an error result
    pub fn error(error: String) -> Self {
        Self {
            success: false,
            output: String::new(),
            error: Some(error),
            metadata: HashMap::new(),
        }
    }

    /// Add metadata
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }
}

/// Tool capability flags
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCapability {
    /// Can read files
    ReadFiles,
    /// Can write files
    WriteFiles,
    /// Can execute shell commands
    ExecuteShell,
    /// Can access network
    Network,
    /// Can modify system state
    SystemModification,
    /// Can manage plans and tasks
    PlanManagement,
}

/// Tool trait - defines an executable tool
#[async_trait]
pub trait Tool: Send + Sync {
    /// Get the tool name
    fn name(&self) -> &str;

    /// Get the tool description
    fn description(&self) -> &str;

    /// Get the input schema (JSON Schema format)
    fn input_schema(&self) -> Value;

    /// Get the tool's capabilities
    fn capabilities(&self) -> Vec<ToolCapability>;

    /// Check if the tool requires approval before execution
    fn requires_approval(&self) -> bool {
        // By default, dangerous tools require approval
        let dangerous_capabilities = [
            ToolCapability::WriteFiles,
            ToolCapability::ExecuteShell,
            ToolCapability::SystemModification,
        ];

        self.capabilities()
            .iter()
            .any(|cap| dangerous_capabilities.contains(cap))
    }

    /// Execute the tool with given input
    async fn execute(&self, input: Value, context: &ToolExecutionContext) -> Result<ToolResult>;

    /// Validate input before execution
    fn validate_input(&self, _input: &Value) -> Result<()> {
        // Default implementation - no validation
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execution_context() {
        let session_id = Uuid::new_v4();
        let ctx = ToolExecutionContext::new(session_id)
            .with_auto_approve(true)
            .with_timeout(60);

        assert_eq!(ctx.session_id, session_id);
        assert!(ctx.auto_approve);
        assert_eq!(ctx.timeout_secs, 60);
    }

    #[test]
    fn test_tool_result_success() {
        let result = ToolResult::success("Done!".to_string())
            .with_metadata("duration_ms".to_string(), "123".to_string());

        assert!(result.success);
        assert_eq!(result.output, "Done!");
        assert!(result.error.is_none());
        assert_eq!(result.metadata.get("duration_ms"), Some(&"123".to_string()));
    }

    #[test]
    fn test_tool_result_error() {
        let result = ToolResult::error("Something went wrong".to_string());

        assert!(!result.success);
        assert_eq!(result.error, Some("Something went wrong".to_string()));
    }
}
