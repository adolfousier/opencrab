//! Session Search Tool
//!
//! Searches chat session message history with a direct case-insensitive
//! SQL LIKE query against the `messages` table. Always up to date and
//! exhaustive (no indexing/truncation), so the agent can find content in
//! its own active session including thousands of messages back.

use super::error::Result;
use super::r#trait::{Tool, ToolCapability, ToolExecutionContext, ToolResult};
use crate::db::Pool;
use async_trait::async_trait;
use serde_json::Value;

/// Tool for listing and searching session message history via direct DB search.
pub struct SessionSearchTool {
    pool: Pool,
}

impl SessionSearchTool {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl Tool for SessionSearchTool {
    fn name(&self) -> &str {
        "session_search"
    }

    fn description(&self) -> &str {
        "Search or list chat session history using direct case-insensitive substring \
         search against the messages table. Always up-to-date and exhaustive. \
         Use 'list' to show all sessions with titles, dates, and message counts. \
         Use 'search' to find messages across sessions by substring query. \
         'session' can be a number (1 = most recent), a title keyword, or 'all' (default)."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": ["list", "search"],
                    "description": "'list' to show sessions, 'search' to find messages"
                },
                "query": {
                    "type": "string",
                    "description": "Natural-language query (required for 'search')"
                },
                "session": {
                    "type": "string",
                    "description": "Session to search: number (1=most recent), title keyword, or 'all' (default)"
                },
                "n": {
                    "type": "integer",
                    "description": "Max results to return (default: 10)",
                    "default": 10
                }
            },
            "required": ["operation"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadFiles]
    }

    fn requires_approval(&self) -> bool {
        false
    }

    async fn execute(&self, input: Value, _context: &ToolExecutionContext) -> Result<ToolResult> {
        let operation = input
            .get("operation")
            .and_then(|v| v.as_str())
            .unwrap_or("list");

        match operation {
            "list" => self.list_sessions().await,
            "search" => {
                let query = match input.get("query").and_then(|v| v.as_str()) {
                    Some(q) if !q.is_empty() => q.to_string(),
                    _ => {
                        return Ok(ToolResult::error(
                            "'query' is required for search".to_string(),
                        ));
                    }
                };
                let session_filter = input
                    .get("session")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                let n = input.get("n").and_then(|v| v.as_u64()).unwrap_or(10) as usize;
                self.search_sessions(&query, session_filter.as_deref(), n)
                    .await
            }
            _ => Ok(ToolResult::error(format!(
                "Unknown operation '{}'. Use 'list' or 'search'.",
                operation
            ))),
        }
    }
}

impl SessionSearchTool {
    async fn list_sessions(&self) -> Result<ToolResult> {
        use crate::db::repository::{MessageRepository, SessionListOptions, SessionRepository};

        let session_repo = SessionRepository::new(self.pool.clone());
        let message_repo = MessageRepository::new(self.pool.clone());

        let sessions = session_repo
            .list(SessionListOptions {
                include_archived: false,
                limit: None,
                offset: 0,
                query: None,
                include_subagents: false,
            })
            .await
            .map_err(|e| super::error::ToolError::Execution(e.to_string()))?;

        if sessions.is_empty() {
            return Ok(ToolResult::success("No sessions found.".to_string()));
        }

        let mut output = String::new();
        for (i, session) in sessions.iter().enumerate() {
            let count = message_repo.count_by_session(session.id).await.unwrap_or(0);
            let title = session.title.as_deref().unwrap_or("Untitled");
            let date = session.updated_at.format("%Y-%m-%d").to_string();
            output.push_str(&format!(
                "{}. \"{}\" — {}, {} messages\n",
                i + 1,
                title,
                date,
                count
            ));
        }

        Ok(ToolResult::success(output))
    }

    async fn search_sessions(
        &self,
        query: &str,
        session_filter: Option<&str>,
        n: usize,
    ) -> Result<ToolResult> {
        use crate::db::repository::{MessageRepository, SessionListOptions, SessionRepository};

        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Ok(ToolResult::error("Query cannot be empty.".to_string()));
        }

        let session_repo = SessionRepository::new(self.pool.clone());
        let message_repo = MessageRepository::new(self.pool.clone());

        // Load all sessions (most-recent-first) to resolve the filter and to
        // map session ids -> titles for output formatting.
        let all_sessions = session_repo
            .list(SessionListOptions {
                include_archived: true,
                limit: None,
                offset: 0,
                query: None,
                include_subagents: false,
            })
            .await
            .map_err(|e| super::error::ToolError::Execution(e.to_string()))?;

        let target_sessions: Vec<_> = match session_filter {
            None | Some("all") => all_sessions.clone(),
            Some(filter) => {
                if let Ok(idx) = filter.parse::<usize>() {
                    all_sessions
                        .get(idx.saturating_sub(1))
                        .cloned()
                        .into_iter()
                        .collect()
                } else {
                    let lower = filter.to_lowercase();
                    all_sessions
                        .iter()
                        .filter(|s| {
                            s.title
                                .as_deref()
                                .unwrap_or("")
                                .to_lowercase()
                                .contains(&lower)
                        })
                        .cloned()
                        .collect()
                }
            }
        };

        if target_sessions.is_empty() {
            return Ok(ToolResult::success(
                "No matching sessions found.".to_string(),
            ));
        }

        // Scope the SQL to the resolved session ids when a filter was given.
        // For "all" / unfiltered, pass None so the query scans every session.
        let scope_ids: Option<Vec<uuid::Uuid>> = match session_filter {
            None | Some("all") => None,
            Some(_) => Some(target_sessions.iter().map(|s| s.id).collect()),
        };

        let messages = message_repo
            .search_by_content(scope_ids.as_deref(), trimmed, n)
            .await
            .map_err(|e| super::error::ToolError::Execution(e.to_string()))?;

        if messages.is_empty() {
            return Ok(ToolResult::success(format!(
                "No messages found matching '{}' in the selected session(s).",
                trimmed
            )));
        }

        let title_map: std::collections::HashMap<uuid::Uuid, String> = all_sessions
            .iter()
            .map(|s| {
                (
                    s.id,
                    s.title.clone().unwrap_or_else(|| "Untitled".to_string()),
                )
            })
            .collect();

        let mut output = String::new();
        for msg in &messages {
            let title = title_map
                .get(&msg.session_id)
                .map(String::as_str)
                .unwrap_or("Untitled");
            let date = msg.created_at.format("%Y-%m-%d %H:%M").to_string();
            let role = if msg.role == "user" {
                "user"
            } else {
                "assistant"
            };
            let snippet = extract_snippet(&msg.content, trimmed, 280);
            output.push_str(&format!(
                "**{}** [{} • {}]\n   {}\n\n",
                title, role, date, snippet
            ));
        }

        Ok(ToolResult::success(output))
    }
}

fn extract_snippet(body: &str, query: &str, max_len: usize) -> String {
    let query_lower = query.to_lowercase();
    let body_lower = body.to_lowercase();

    let best_pos = body_lower.find(&query_lower).unwrap_or(0);

    let start = best_pos.saturating_sub(50);
    let end = (start + max_len).min(body.len());
    let start = body.floor_char_boundary(start);
    let end = body.ceil_char_boundary(end);

    let mut snippet = String::new();
    if start > 0 {
        snippet.push_str("...");
    }
    snippet.push_str(body[start..end].trim());
    if end < body.len() {
        snippet.push_str("...");
    }

    // Collapse runs of whitespace so multi-line content stays readable in the
    // single-line snippet output.
    snippet.split_whitespace().collect::<Vec<_>>().join(" ")
}
