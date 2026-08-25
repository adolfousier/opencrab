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
         Use 'tail' to read the last N messages of one session (cheap on huge histories). \
         Use 'query' for machine-readable session discovery: JSON rows with full session ids, \
         titles, last-active timestamps and message counts - e.g. to find another session's id \
         to target with session_notify. \
         'session' can be a number (1 = most recent), a title keyword, or 'all' (default; \
         ignored by 'tail', which falls back to the newest session)."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": ["list", "search", "tail"],
                    "description": "'list' to show sessions, 'search' to find messages, 'tail' to read the last N messages of a session"
                },
                "query": {
                    "type": "string",
                    "description": "Natural-language query (required for 'search')"
                },
                "session": {
                    "type": "string",
                    "description": "Session to search: number (1=most recent), title keyword, or 'all' (default)"
                },
                "status": {
                    "type": "string",
                    "enum": ["active", "archived", "all"],
                    "description": "Session state filter for 'query' (default: active)"
                },
                "title_contains": {
                    "type": "string",
                    "description": "Case-insensitive title substring filter for 'query'"
                },
                "updated_since": {
                    "type": "string",
                    "description": "'query' only: sessions last active after this point. RFC3339 timestamp or Nd/Nh shorthand (e.g. 7d, 24h)"
                },
                "limit": {
                    "type": "integer",
                    "description": "'query' only: max rows returned (default: 50, max: 500)"
                },
                "n": {
                    "type": "integer",
                    "description": "Max results for 'search'/'tail' (default: 10)",
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
            "tail" => {
                let session_filter = input
                    .get("session")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                // Default 10, clamped to 100 so a stray huge n can't dump a
                // whole history into context.
                let n = input
                    .get("n")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(10)
                    .clamp(1, 100) as usize;
                self.tail_session(session_filter.as_deref(), n).await
            }
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
                "Unknown operation '{}'. Use 'list', 'search', or 'tail'.",
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

        let mut output =
            String::from("Sessions, newest first ([short-id] identifies each row):\n");
        for (i, session) in sessions.iter().enumerate() {
            let count = message_repo.count_by_session(session.id).await.unwrap_or(0);
            let title = session.title.as_deref().unwrap_or("Untitled");
            let date = session.updated_at.format("%Y-%m-%d %H:%M").to_string();
            output.push_str(&format!(
                "{}. [{}] \"{}\" — last active {}, {} messages\n",
                i + 1,
                &session.id.to_string()[..8],
                title,
                date,
                count
            ));
        }

        Ok(ToolResult::success(output))
    }

    /// Machine-readable session discovery.
    ///
    /// Returns JSON rows `{id, title, last_active, messages}` with FULL UUIDs
    /// so other tools (`session_notify`) can be targeted without parsing the
    /// human-oriented `list` output. Rows are ordered by last activity,
    /// newest first (repo-level ORDER BY).
    async fn query_sessions(
        &self,
        status: &str,
        title_contains: Option<&str>,
        updated_since: Option<chrono::DateTime<chrono::Utc>>,
        limit: usize,
    ) -> Result<ToolResult> {
        use crate::db::repository::{MessageRepository, SessionListOptions, SessionRepository};

        let session_repo = SessionRepository::new(self.pool.clone());
        let message_repo = MessageRepository::new(self.pool.clone());

        // Repo sorts by updated_at DESC already; archived/all need the wider
        // SQL net, then a Rust-side filter narrows to archived-only rows.
        let include_archived = status != "active";
        let sessions = session_repo
            .list(SessionListOptions {
                include_archived,
                limit: None,
                offset: 0,
                query: title_contains.map(str::to_string),
                include_subagents: false,
            })
            .await
            .map_err(|e| super::error::ToolError::Execution(e.to_string()))?;

        let mut selected: Vec<_> = sessions
            .into_iter()
            .filter(|s| status != "archived" || s.archived_at.is_some())
            .filter(|s| updated_since.map_or(true, |since| s.updated_at > since))
            .collect();

        if selected.is_empty() {
            return Ok(ToolResult::success("[]".to_string()));
        }
        selected.truncate(limit);

        let mut rows = Vec::with_capacity(selected.len());
        for s in &selected {
            let count = message_repo.count_by_session(s.id).await.unwrap_or(0);
            rows.push(serde_json::json!({
                "id": s.id.to_string(),
                "title": s.title.as_deref().unwrap_or("Untitled"),
                "last_active": s
                    .updated_at
                    .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                "messages": count,
            }));
        }

        let json = serde_json::to_string_pretty(&rows).unwrap_or_else(|_| "[]".to_string());
        Ok(ToolResult::success(json))
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

    /// Read the last `n` messages of one session, oldest-first.
    ///
    /// Filter resolution mirrors `search_sessions` (number = position in the
    /// most-recent-first list, else title keyword). With no filter, the
    /// newest session is used. 'all' is rejected: tail needs one session.
    async fn tail_session(&self, session_filter: Option<&str>, n: usize) -> Result<ToolResult> {
        use crate::db::repository::{MessageRepository, SessionListOptions, SessionRepository};

        if session_filter == Some("all") {
            return Ok(ToolResult::error(
                "'tail' needs a single session. Pass a session number (see 'list'), \
                 a title keyword, or no 'session' to tail the newest one."
                    .to_string(),
            ));
        }

        let session_repo = SessionRepository::new(self.pool.clone());
        let message_repo = MessageRepository::new(self.pool.clone());

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

        if all_sessions.is_empty() {
            return Ok(ToolResult::success("No sessions found.".to_string()));
        }

        let target = match session_filter {
            None => all_sessions.first().cloned(),
            Some(filter) => {
                // Raw session id (spec: "session id or newest default").
                if let Ok(id) = filter.parse::<uuid::Uuid>() {
                    all_sessions.iter().find(|s| s.id == id).cloned()
                } else if let Ok(idx) = filter.parse::<usize>() {
                    all_sessions.get(idx.saturating_sub(1)).cloned()
                } else {
                    let lower = filter.to_lowercase();
                    all_sessions
                        .iter()
                        .find(|s| {
                            s.title
                                .as_deref()
                                .unwrap_or("")
                                .to_lowercase()
                                .contains(&lower)
                        })
                        .cloned()
                }
            }
        };

        let Some(session) = target else {
            return Ok(ToolResult::success(format!(
                "No matching session found for '{}'. Use 'list' to see sessions.",
                session_filter.unwrap_or("newest")
            )));
        };

        let messages = message_repo
            .find_recent_by_session(session.id, n)
            .await
            .map_err(|e| super::error::ToolError::Execution(e.to_string()))?;

        let title = session.title.as_deref().unwrap_or("Untitled");
        if messages.is_empty() {
            return Ok(ToolResult::success(format!(
                "Session \"{}\" has no messages.",
                title
            )));
        }

        let mut output = format!(
            "Last {} of {} messages in \"{}\":\n\n",
            messages.len(),
            message_repo.count_by_session(session.id).await.unwrap_or(0),
            title
        );
        for msg in &messages {
            let date = msg.created_at.format("%Y-%m-%d %H:%M").to_string();
            let role = if msg.role == "user" {
                "user"
            } else {
                "assistant"
            };
            let body: String = msg.content.split_whitespace().collect::<Vec<_>>().join(" ");
            let body = if body.len() > 500 {
                let mut end = 500.min(body.len());
                while !body.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{}...", &body[..end])
            } else {
                body
            };
            output.push_str(&format!("[{}] {}: {}\n\n", date, role, body));
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

/// Parse an `updated_since` spec: RFC3339 timestamp, or `Nd`/`Nh` shorthand
/// meaning N days/hours back from now.
fn parse_updated_since(
    spec: &str,
) -> std::result::Result<chrono::DateTime<chrono::Utc>, String> {
    let spec = spec.trim();
    let invalid = || {
        format!(
            "Invalid updated_since '{}': use RFC3339 (2026-08-25T00:00:00Z) or Nd/Nh shorthand (7d, 24h)",
            spec
        )
    };
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(spec) {
        return Ok(dt.with_timezone(&chrono::Utc));
    }
    if spec.len() < 2 {
        return Err(invalid());
    }
    let (num, unit) = spec.split_at(spec.len() - 1);
    let Ok(n) = num.parse::<i64>() else {
        return Err(invalid());
    };
    let duration = match (unit, n) {
        ("h", n) if n > 0 => chrono::Duration::hours(n),
        ("d", n) if n > 0 => chrono::Duration::days(n),
        _ => return Err(invalid()),
    };
    Ok(chrono::Utc::now() - duration)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rfc3339() {
        let dt = parse_updated_since("2026-08-25T00:00:00Z").expect("valid rfc3339");
        assert_eq!(dt.to_rfc3339(), "2026-08-25T00:00:00+00:00");
    }

    #[test]
    fn parses_shorthand() {
        let before = chrono::Utc::now();
        let dt = parse_updated_since("24h").expect("24h valid");
        assert!(dt > before - chrono::Duration::hours(25));
        assert!(dt <= chrono::Utc::now());

        let dt = parse_updated_since("7d").expect("7d valid");
        assert!(dt < before - chrono::Duration::days(6));
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_updated_since("yesterday").is_err());
        assert!(parse_updated_since("").is_err());
        assert!(parse_updated_since("12x").is_err());
        assert!(parse_updated_since("-5d").is_err());
    }
}
