//! Web Search Tool
//!
//! Perform real-time internet searches and retrieve results.

use super::error::{Result, ToolError};
use super::r#trait::{Tool, ToolCapability, ToolExecutionContext, ToolResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Web search tool
pub struct WebSearchTool;

#[derive(Debug, Deserialize, Serialize)]
struct SearchInput {
    /// Search query
    query: String,

    /// Maximum number of results to return
    #[serde(default = "default_max_results")]
    max_results: usize,
}

fn default_max_results() -> usize {
    5
}

// DuckDuckGo HTML search result
#[derive(Debug, Deserialize)]
pub(crate) struct SearchResult {
    pub(crate) title: String,
    pub(crate) url: String,
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Search the internet for real-time information using DuckDuckGo. \
         Returns summarized results with links. \
         \n\nDEFAULT web-research tool — use this for any \"find me info \
         about X\" / \"what's the latest Y\" / \"check the docs for Z\" \
         request unless the user explicitly asks for browser interaction. \
         Always pick a search tool over `browser_navigate` for research. \
         \n\nIf `exa_search` or `brave_search` are also in your tool list, \
         prefer them over `web_search` (better ranking for technical / \
         current-events queries respectively); `web_search` is the \
         always-available fallback. For GitHub content (issues, PRs, \
         repos, code search) use the `gh` CLI via `bash` instead — it \
         returns structured JSON and is authenticated."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query (e.g., 'latest Node.js LTS release', 'Rust async programming')"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of results to return (default: 5)",
                    "default": 5,
                    "minimum": 1,
                    "maximum": 10
                }
            },
            "required": ["query"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::Network]
    }

    fn requires_approval(&self) -> bool {
        false // Web search is generally safe (read-only)
    }

    fn validate_input(&self, input: &Value) -> Result<()> {
        let input: SearchInput = serde_json::from_value(input.clone())
            .map_err(|e| ToolError::InvalidInput(format!("Invalid input: {}", e)))?;

        if input.query.trim().is_empty() {
            return Err(ToolError::InvalidInput("Query cannot be empty".to_string()));
        }

        if input.max_results == 0 || input.max_results > 10 {
            return Err(ToolError::InvalidInput(
                "max_results must be between 1 and 10".to_string(),
            ));
        }

        Ok(())
    }

    async fn execute(&self, input: Value, _context: &ToolExecutionContext) -> Result<ToolResult> {
        let input: SearchInput = serde_json::from_value(input)?;

        // Use DuckDuckGo Lite endpoint which returns actual web search results
        let url = format!(
            "https://lite.duckduckgo.com/lite/?q={}",
            urlencoding::encode(&input.query)
        );

        // DuckDuckGo Lite 403s on IP/UA reputation and rate limits (#525): a
        // single hardcoded User-Agent used to fail outright. Rotate through a
        // small pool of realistic UAs, retrying on a 403 / transient failure
        // with a short backoff before giving up. A hard failure returns an
        // actionable error so the agent falls back to exa_search / brave_search
        // / http_request, exactly as the tool description already instructs.
        let mut last_err = String::new();
        for (attempt, ua) in DDG_USER_AGENTS.iter().enumerate() {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .user_agent(*ua)
                .build()
                .map_err(|e| ToolError::Execution(format!("Failed to create HTTP client: {e}")))?;

            match client.get(&url).send().await {
                // DDG bot-detection challenge: HTTP 202 is their
                // protocol-level "you're blocked" signal. Stable across
                // deploys. Short-circuit instead of burning UA rotations.
                Ok(response) if response.status().as_u16() == 202 => {
                    return Ok(ToolResult::error(
                        "DuckDuckGo returned a bot-detection challenge (HTTP 202). \
                         Try exa_search or brave_search instead."
                            .to_string(),
                    ));
                }
                Ok(response) if response.status().is_success() => {
                    let html = response.text().await.map_err(|e| {
                        ToolError::Execution(format!("Failed to read response: {e}"))
                    })?;
                    let results = parse_lite_results(&html, input.max_results);
                    // Zero results + form present = challenge page, not a
                    // genuine empty result. Structural check, no reliance on
                    // internal JS filenames DDG can rename at any time.
                    if results.is_empty() && html.contains("<form") {
                        return Ok(ToolResult::error(
                            "DuckDuckGo returned a captcha/challenge page instead of \
                             results. Try exa_search or brave_search instead."
                                .to_string(),
                        ));
                    }
                    return Ok(ToolResult::success(format_results(&input.query, &results)));
                }
                Ok(response) => {
                    last_err = format!("HTTP {}", response.status());
                    tracing::warn!(
                        "web_search: DuckDuckGo returned {} (UA {}/{}) — rotating User-Agent",
                        response.status(),
                        attempt + 1,
                        DDG_USER_AGENTS.len(),
                    );
                }
                Err(e) => {
                    last_err = e.to_string();
                    tracing::warn!(
                        "web_search: request failed (UA {}/{}): {e}",
                        attempt + 1,
                        DDG_USER_AGENTS.len(),
                    );
                }
            }

            // Brief backoff before the next UA (skipped after the last attempt).
            if attempt + 1 < DDG_USER_AGENTS.len() {
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
        }

        Ok(ToolResult::error(format!(
            "DuckDuckGo search failed after {} attempts (last error: {last_err}). DuckDuckGo may \
             be rate-limiting or blocking this IP. Fall back to exa_search or brave_search if they \
             are in your tool list, otherwise use http_request against a search API.",
            DDG_USER_AGENTS.len(),
        )))
    }
}

/// Rotated User-Agent pool for DuckDuckGo Lite (#525). DDG blocks repetitive UA
/// patterns from one IP, so trying a few realistic browser UAs recovers most
/// transient 403s.
pub(crate) const DDG_USER_AGENTS: &[&str] = &[
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.3 Safari/605.1.15",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:123.0) Gecko/20100101 Firefox/123.0",
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/121.0.0.0 Safari/537.36",
];

/// Format parsed search results into the tool's human-readable output.
pub(crate) fn format_results(query: &str, results: &[SearchResult]) -> String {
    let mut output = String::new();
    output.push_str(&format!("🔍 Search results for: \"{query}\"\n\n"));
    if results.is_empty() {
        output.push_str("ℹ️  No results found. Try:\n");
        output.push_str("  • Rephrasing your query\n");
        output.push_str("  • Using more specific keywords\n");
        output.push_str("  • Searching for a different topic\n");
    } else {
        for (i, result) in results.iter().enumerate() {
            output.push_str(&format!("{}. {}\n", i + 1, result.title));
            output.push_str(&format!("   🔗 {}\n\n", result.url));
        }
    }
    output
}

/// Parse DuckDuckGo Lite HTML response to extract search results
pub(crate) fn parse_lite_results(html: &str, max_results: usize) -> Vec<SearchResult> {
    let mut results = Vec::new();

    // Find result links in the HTML
    // DDG Lite uses <a> tags with class "result-link" for search results
    let link_regex =
        regex::Regex::new(r#"<a[^>]*class="result-link"[^>]*href="([^"]*)"[^>]*>([^<]*)</a>"#)
            .unwrap_or_else(|_| {
                regex::Regex::new(r#"<a[^>]*href="([^"]*)"[^>]*>([^<]*)</a>"#).unwrap()
            });

    for cap in link_regex.captures_iter(html) {
        if results.len() >= max_results {
            break;
        }

        let url = cap
            .get(1)
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();
        let title = cap
            .get(2)
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();

        // Skip non-http links and empty titles
        if url.starts_with("http") && !title.trim().is_empty() {
            results.push(SearchResult { title, url });
        }
    }

    // Fallback: if no results found with class="result-link", try generic link parsing
    if results.is_empty() {
        let generic_regex =
            regex::Regex::new(r#"<a[^>]*href="(https?://[^"]*)"[^>]*>([^<]{10,})</a>"#).unwrap();

        for cap in generic_regex.captures_iter(html) {
            if results.len() >= max_results {
                break;
            }

            let url = cap
                .get(1)
                .map(|m| m.as_str().to_string())
                .unwrap_or_default();
            let title = cap
                .get(2)
                .map(|m| m.as_str().to_string())
                .unwrap_or_default();

            // Skip duckduckgo own links
            if !url.contains("duckduckgo.com") && !title.trim().is_empty() {
                results.push(SearchResult { title, url });
            }
        }
    }

    results
}
