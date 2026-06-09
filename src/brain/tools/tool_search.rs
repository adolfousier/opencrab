//! `tool_search` — the agent's on-demand tool discovery.
//!
//! In lazy-tools mode the request carries only the CORE schemas plus this
//! tool. When the agent needs anything else (browser, channels, sub-agents,
//! media, system, …) it calls `tool_search("what I need")`; the matching
//! tools are activated for the session (so their schemas ride on subsequent
//! requests) and their full schemas are returned immediately so the agent can
//! act right away. Mirrors `load_brain_file` for contextual brain files.

use super::error::Result;
use super::registry::ToolRegistry;
use super::r#trait::{Tool, ToolCapability, ToolExecutionContext, ToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

/// Max tools returned (and activated) per search — keeps a single discovery
/// from re-injecting a huge chunk of the schemas it was meant to avoid.
const MAX_RESULTS: usize = 8;

pub struct ToolSearchTool {
    registry: Arc<ToolRegistry>,
}

impl ToolSearchTool {
    pub fn new(registry: Arc<ToolRegistry>) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl Tool for ToolSearchTool {
    fn name(&self) -> &str {
        super::catalog::TOOL_SEARCH_NAME
    }

    fn description(&self) -> &str {
        "Discover and activate tools beyond your always-available core set. You start each turn \
         with core tools (file I/O, shell, search, task/plan/context, http, the brain-file loader, \
         config/session basics). For ANYTHING else — browsing/clicking web pages, sending channel \
         messages (Telegram/Discord/Slack/WhatsApp), spawning sub-agents or teams, generating or \
         analyzing images/video, cron jobs, self-improvement/rebuild/evolve — call this FIRST with \
         a short description of what you need. It returns the matching tools' exact schemas and \
         makes them callable for the rest of this session. If a task needs a tool you don't see, \
         search for it here before saying you can't do it."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Plain-words description of what you need to do (e.g. 'send a telegram photo', 'click a button on a web page', 'spawn a sub-agent', 'generate an image', 'create a cron job'). May also be a category: browser, channels, agents, media, system, utility."
                }
            },
            "required": ["query"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        // Read-only discovery: searches the registry, activates schemas. No
        // file/shell/network side effects, so no approval gate.
        vec![]
    }

    async fn execute(&self, input: Value, context: &ToolExecutionContext) -> Result<ToolResult> {
        let query = input
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if query.is_empty() {
            return Ok(ToolResult::error(
                "tool_search needs a 'query' — describe what you need to do (e.g. 'send a telegram message').".to_string(),
            ));
        }

        let matches = self.registry.search_tools(query, MAX_RESULTS);
        if matches.is_empty() {
            return Ok(ToolResult::success(format!(
                "No additional tools matched \"{query}\". Your core tools may already cover it; \
                 otherwise try different words or a category (browser, channels, agents, media, \
                 system, utility)."
            )));
        }

        // Activate the matches for this session so their schemas ship on the
        // next request, then hand back the full schemas for immediate use.
        let names: std::collections::HashSet<String> =
            matches.iter().map(|(n, ..)| n.clone()).collect();
        self.registry
            .activate_tools(context.session_id, names.clone());
        let defs = self.registry.definitions_for(&names);

        let mut out = format!(
            "Activated {} tool(s) for this session — they are now callable (call them directly):\n\n",
            defs.len()
        );
        for def in &defs {
            out.push_str(&format!(
                "### {} [{}]\n{}\nschema: {}\n\n",
                def.name,
                super::catalog::tool_category(&def.name),
                def.description,
                serde_json::to_string(&def.input_schema).unwrap_or_default(),
            ));
        }
        Ok(ToolResult::success(out))
    }
}
