//! Tool Registry
//!
//! Manages the collection of available tools that can be invoked by agents.

use super::error::{Result, ToolError};
use super::r#trait::{Tool, ToolExecutionContext, ToolResult};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Per-tool parameter aliases that LLMs commonly confuse.
/// Format: (tool_name, wrong_param, correct_param).
/// Applied before validation so models that send slight variations still work.
const PARAM_ALIASES: &[(&str, &str, &str)] = &[
    // grep/glob: LLMs often send "query" instead of "pattern"
    ("grep", "query", "pattern"),
    ("glob", "query", "pattern"),
    // file tools: "file", "file_path", "filepath" → "path"
    ("read_file", "file", "path"),
    ("read_file", "file_path", "path"),
    ("read_file", "filepath", "path"),
    ("write_file", "file", "path"),
    ("write_file", "file_path", "path"),
    ("write_file", "filepath", "path"),
    ("edit_file", "file", "path"),
    ("edit_file", "file_path", "path"),
    ("edit_file", "filepath", "path"),
    // edit_file: Claude Code sends old_string/new_string → old_text/new_text
    ("edit_file", "old_string", "old_text"),
    ("edit_file", "new_string", "new_text"),
    ("doc_parser", "file", "path"),
    ("doc_parser", "file_path", "path"),
    // write: "text", "body" → "content"
    ("write_file", "text", "content"),
    ("write_file", "body", "content"),
    // bash: "cmd" → "command"
    ("bash", "cmd", "command"),
    // search tools: "pattern" → "query"
    ("web_search", "pattern", "query"),
    ("exa_search", "pattern", "query"),
    ("brave_search", "pattern", "query"),
    ("memory_search", "pattern", "query"),
];

/// Normalize tool input by mapping common LLM parameter name mistakes
/// to the correct parameter name. Only remaps if the correct name is absent.
fn normalize_tool_input(tool_name: &str, mut input: Value) -> Value {
    if let Some(obj) = input.as_object_mut() {
        for &(tool, wrong, correct) in PARAM_ALIASES {
            if tool == tool_name
                && !obj.contains_key(correct)
                && let Some(val) = obj.remove(wrong)
            {
                tracing::debug!(
                    "Normalized tool param: {}.{} → {}.{}",
                    tool_name,
                    wrong,
                    tool_name,
                    correct
                );
                obj.insert(correct.to_string(), val);
            }
        }
    }
    input
}

/// Registry of available tools.
///
/// Thread-safe via internal `RwLock` — all methods take `&self`, allowing
/// runtime registration/removal through a shared `Arc<ToolRegistry>`.
pub struct ToolRegistry {
    tools: RwLock<HashMap<String, Arc<dyn Tool>>>,
    /// EXTENDED tools each session has activated via `tool_search` (lazy-tools
    /// mode). Lives here because both the agent loop and the `tool_search`
    /// tool already share this registry's `Arc` — no separate plumbing.
    session_active: RwLock<HashMap<uuid::Uuid, std::collections::HashSet<String>>>,
}

impl ToolRegistry {
    /// Create a new empty tool registry
    pub fn new() -> Self {
        Self {
            tools: RwLock::new(HashMap::new()),
            session_active: RwLock::new(HashMap::new()),
        }
    }

    /// Mark EXTENDED tools as active for a session, so subsequent requests
    /// include their schemas. Called by `tool_search` after a discovery.
    pub fn activate_tools(&self, session_id: uuid::Uuid, names: impl IntoIterator<Item = String>) {
        let mut map = self.session_active.write().unwrap();
        map.entry(session_id).or_default().extend(names);
    }

    /// The EXTENDED tools currently active for a session (empty if none yet).
    pub fn active_tools(&self, session_id: uuid::Uuid) -> std::collections::HashSet<String> {
        self.session_active
            .read()
            .unwrap()
            .get(&session_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Register a tool (takes `&self` — safe through shared `Arc`)
    pub fn register(&self, tool: Arc<dyn Tool>) {
        let name = tool.name().to_string();
        tracing::debug!("Registered tool: {}", name);
        self.tools.write().unwrap().insert(name, tool);
    }

    /// Unregister a tool by name. Returns true if it existed.
    pub fn unregister(&self, name: &str) -> bool {
        self.tools.write().unwrap().remove(name).is_some()
    }

    /// Get a tool by name
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.read().unwrap().get(name).cloned()
    }

    /// Check if a tool is registered
    pub fn has_tool(&self, name: &str) -> bool {
        self.tools.read().unwrap().contains_key(name)
    }

    /// List all registered tool names
    pub fn list_tools(&self) -> Vec<String> {
        self.tools.read().unwrap().keys().cloned().collect()
    }

    /// Get tool definitions in LLM format
    pub fn get_tool_definitions(&self) -> Vec<crate::brain::provider::Tool> {
        self.tools
            .read()
            .unwrap()
            .values()
            .map(|tool| crate::brain::provider::Tool {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                input_schema: tool.input_schema(),
            })
            .collect()
    }

    /// Lazy-tools mode: only the schemas for the CORE set plus any EXTENDED
    /// tools the session has activated via `tool_search`. Keeps a "reply yes"
    /// turn from shipping all ~95 schemas (~20k tokens) when it needs none.
    /// A tool the agent hasn't discovered yet is simply omitted — calling
    /// `tool_search` activates it for subsequent requests.
    pub fn get_tool_definitions_filtered(
        &self,
        active_extended: &std::collections::HashSet<String>,
    ) -> Vec<crate::brain::provider::Tool> {
        use crate::brain::tools::catalog;
        self.tools
            .read()
            .unwrap()
            .values()
            .filter(|tool| {
                let name = tool.name();
                catalog::is_core(name) || active_extended.contains(name)
            })
            .map(|tool| crate::brain::provider::Tool {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                input_schema: tool.input_schema(),
            })
            .collect()
    }

    /// Find EXTENDED (non-core) tools matching a free-text query, ranked by
    /// how well the query terms hit the tool's name + description. Powers the
    /// `tool_search` discovery tool. Returns `(name, category, description)`,
    /// best matches first, capped at `limit`.
    pub fn search_tools(&self, query: &str, limit: usize) -> Vec<(String, String, String)> {
        use crate::brain::tools::catalog;
        let q = query.to_ascii_lowercase();
        let terms: Vec<&str> = q.split_whitespace().filter(|t| t.len() > 1).collect();
        let mut scored: Vec<(i32, String, String, String)> = self
            .tools
            .read()
            .unwrap()
            .values()
            .filter(|tool| !catalog::is_core(tool.name()))
            .map(|tool| {
                let name = tool.name().to_string();
                let desc = tool.description().to_string();
                let category = catalog::tool_category(&name).to_string();
                let hay = format!("{name} {category} {desc}").to_ascii_lowercase();
                let mut score = 0i32;
                for term in &terms {
                    if name.to_ascii_lowercase().contains(term) {
                        score += 5; // name hit is the strongest signal
                    } else if category.contains(term) {
                        score += 3;
                    } else if hay.contains(term) {
                        score += 1;
                    }
                }
                (score, name, category, desc)
            })
            .filter(|(score, ..)| *score > 0)
            .collect();
        // Highest score first; stable tie-break by name for determinism.
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        scored
            .into_iter()
            .take(limit)
            .map(|(_, name, category, desc)| (name, category, desc))
            .collect()
    }

    /// Full provider-format definitions for a specific set of tool names (used
    /// by `tool_search` to hand the agent the exact schemas it just discovered).
    pub fn definitions_for(
        &self,
        names: &std::collections::HashSet<String>,
    ) -> Vec<crate::brain::provider::Tool> {
        self.tools
            .read()
            .unwrap()
            .values()
            .filter(|tool| names.contains(tool.name()))
            .map(|tool| crate::brain::provider::Tool {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                input_schema: tool.input_schema(),
            })
            .collect()
    }

    /// Execute a tool by name
    pub async fn execute(
        &self,
        name: &str,
        input: Value,
        context: &ToolExecutionContext,
    ) -> Result<ToolResult> {
        // Resolve the tool. On an unknown name, try the tool-name self-heal
        // (a weaker model guessing `tg_send_message` for `telegram_send`,
        // issue #176) before giving up. The healed name is used for param
        // normalization and logging so everything downstream sees the real
        // tool.
        let (tool, resolved_name) = match self.get(name) {
            Some(t) => (t, name.to_string()),
            None => {
                let registered = self.list_tools();
                match super::tool_name_heal::resolve_tool_name(name, &registered) {
                    Some(real) => {
                        tracing::warn!(
                            "Self-healed tool name: '{}' → '{}' (model called a near-miss name)",
                            name,
                            real
                        );
                        let t = self
                            .get(&real)
                            .ok_or_else(|| ToolError::NotFound(name.to_string()))?;
                        (t, real)
                    }
                    None => return Err(ToolError::NotFound(name.to_string())),
                }
            }
        };
        let name = resolved_name.as_str();

        // JIT discovery (#214): when lazy_tools is on, a tool the model called
        // by name but never surfaced via `tool_search` is absent from the
        // system prompt, so the model is guessing its params blind. Activate it
        // now, BEFORE validation, so even if THIS call fails on bad params the
        // next request carries the real schema and the model self-corrects
        // instead of looping. No-op for CORE tools (always present); harmless
        // when lazy_tools is off (the active set is never consulted).
        if !crate::brain::tools::catalog::is_core(name) {
            self.activate_tools(context.session_id, [name.to_string()]);
        }

        // Normalize LLM parameter name mistakes before validation
        let input = normalize_tool_input(name, input);

        // Validate input
        tool.validate_input(&input)?;

        // Check if approval is required
        if tool.requires_approval() && !context.auto_approve {
            return Err(ToolError::ApprovalRequired(format!(
                "Tool '{}' requires approval before execution",
                name
            )));
        }

        // Execute the tool
        tracing::info!("Executing tool: {}", name);
        let result = tool.execute(input, context).await?;

        if result.success {
            tracing::info!("Tool '{}' executed successfully", name);
        } else {
            tracing::warn!(
                "Tool '{}' failed: {:?}",
                name,
                result.error.as_deref().unwrap_or("unknown error")
            );
        }

        Ok(result)
    }

    /// Get the number of registered tools
    pub fn count(&self) -> usize {
        self.tools.read().unwrap().len()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::tools::r#trait::ToolCapability;
    use async_trait::async_trait;
    use uuid::Uuid;

    /// Mock tool for testing
    struct MockTool {
        name: String,
        requires_approval: bool,
    }

    #[async_trait]
    impl Tool for MockTool {
        fn name(&self) -> &str {
            &self.name
        }

        fn description(&self) -> &str {
            "A mock tool for testing"
        }

        fn input_schema(&self) -> Value {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "Test message"
                    }
                },
                "required": ["message"]
            })
        }

        fn capabilities(&self) -> Vec<ToolCapability> {
            vec![ToolCapability::ReadFiles]
        }

        fn requires_approval(&self) -> bool {
            self.requires_approval
        }

        async fn execute(
            &self,
            _input: Value,
            _context: &ToolExecutionContext,
        ) -> Result<ToolResult> {
            Ok(ToolResult::success("Mock execution successful".to_string()))
        }
    }

    #[test]
    fn test_registry_creation() {
        let registry = ToolRegistry::new();
        assert_eq!(registry.count(), 0);
    }

    #[test]
    fn test_register_tool() {
        let registry = ToolRegistry::new();
        let tool = Arc::new(MockTool {
            name: "test_tool".to_string(),
            requires_approval: false,
        });

        registry.register(tool);
        assert_eq!(registry.count(), 1);
        assert!(registry.has_tool("test_tool"));
        assert!(!registry.has_tool("nonexistent"));
    }

    #[test]
    fn test_list_tools() {
        let registry = ToolRegistry::new();

        registry.register(Arc::new(MockTool {
            name: "tool1".to_string(),
            requires_approval: false,
        }));
        registry.register(Arc::new(MockTool {
            name: "tool2".to_string(),
            requires_approval: false,
        }));

        let tools = registry.list_tools();
        assert_eq!(tools.len(), 2);
        assert!(tools.contains(&"tool1".to_string()));
        assert!(tools.contains(&"tool2".to_string()));
    }

    #[tokio::test]
    async fn test_execute_tool() {
        let registry = ToolRegistry::new();
        let tool = Arc::new(MockTool {
            name: "test_tool".to_string(),
            requires_approval: false,
        });

        registry.register(tool);

        let session_id = Uuid::new_v4();
        let context = ToolExecutionContext::new(session_id);
        let input = serde_json::json!({ "message": "test" });

        let result = registry
            .execute("test_tool", input, &context)
            .await
            .unwrap();
        assert!(result.success);
        assert_eq!(result.output, "Mock execution successful");
    }

    #[tokio::test]
    async fn test_execute_nonexistent_tool() {
        let registry = ToolRegistry::new();
        let session_id = Uuid::new_v4();
        let context = ToolExecutionContext::new(session_id);
        let input = serde_json::json!({});

        let result = registry.execute("nonexistent", input, &context).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ToolError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_execute_requires_approval() {
        let registry = ToolRegistry::new();
        let tool = Arc::new(MockTool {
            name: "dangerous_tool".to_string(),
            requires_approval: true,
        });

        registry.register(tool);

        let session_id = Uuid::new_v4();
        let context = ToolExecutionContext::new(session_id); // auto_approve = false
        let input = serde_json::json!({ "message": "test" });

        let result = registry.execute("dangerous_tool", input, &context).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ToolError::ApprovalRequired(_)
        ));
    }

    /// Mock tool whose input validation always fails — used to prove JIT
    /// activation (#214) runs BEFORE validation, so a tool that fails its
    /// first (blind) call is still discovered for the next turn.
    struct ValidateFailTool;

    #[async_trait]
    impl Tool for ValidateFailTool {
        fn name(&self) -> &str {
            "extended_blind_tool"
        }
        fn description(&self) -> &str {
            "always fails validation"
        }
        fn input_schema(&self) -> Value {
            serde_json::json!({ "type": "object" })
        }
        fn capabilities(&self) -> Vec<ToolCapability> {
            vec![ToolCapability::ReadFiles]
        }
        fn validate_input(&self, _input: &Value) -> Result<()> {
            Err(ToolError::InvalidInput("missing required param".into()))
        }
        async fn execute(
            &self,
            _input: Value,
            _context: &ToolExecutionContext,
        ) -> Result<ToolResult> {
            Ok(ToolResult::success("unreachable".to_string()))
        }
    }

    #[tokio::test]
    async fn test_execute_jit_activates_extended_tool_even_on_failure() {
        // #214: an EXTENDED tool called by name but never surfaced via
        // tool_search must be activated for the session so its schema rides the
        // NEXT request, and that activation has to happen even when this first
        // call fails on bad params, or the model stays stuck guessing blind.
        let registry = ToolRegistry::new();
        registry.register(Arc::new(ValidateFailTool));

        let session_id = Uuid::new_v4();
        let context = ToolExecutionContext::new(session_id);

        assert!(
            !registry
                .active_tools(session_id)
                .contains("extended_blind_tool")
        );

        let result = registry
            .execute("extended_blind_tool", serde_json::json!({}), &context)
            .await;

        // The call failed validation...
        assert!(matches!(result.unwrap_err(), ToolError::InvalidInput(_)));
        // ...yet the tool is now discovered for the next turn.
        assert!(
            registry
                .active_tools(session_id)
                .contains("extended_blind_tool"),
            "extended tool must be activated before validation, even on a failing call"
        );
    }

    #[tokio::test]
    async fn test_execute_does_not_activate_core_tool() {
        // CORE tools are always in the prompt, so JIT activation must skip them
        // (no point bloating the per-session active set). A MockTool registered
        // under a core name (`bash`) must NOT be added to the active set.
        let registry = ToolRegistry::new();
        registry.register(Arc::new(MockTool {
            name: "bash".to_string(),
            requires_approval: false,
        }));

        let session_id = Uuid::new_v4();
        let context = ToolExecutionContext::new(session_id);

        let result = registry
            .execute("bash", serde_json::json!({ "message": "hi" }), &context)
            .await
            .unwrap();
        assert!(result.success);
        assert!(
            !registry.active_tools(session_id).contains("bash"),
            "core tools must never be added to the session active set"
        );
    }

    #[tokio::test]
    async fn test_execute_with_auto_approve() {
        let registry = ToolRegistry::new();
        let tool = Arc::new(MockTool {
            name: "dangerous_tool".to_string(),
            requires_approval: true,
        });

        registry.register(tool);

        let session_id = Uuid::new_v4();
        let context = ToolExecutionContext::new(session_id).with_auto_approve(true);
        let input = serde_json::json!({ "message": "test" });

        let result = registry
            .execute("dangerous_tool", input, &context)
            .await
            .unwrap();
        assert!(result.success);
    }
}
