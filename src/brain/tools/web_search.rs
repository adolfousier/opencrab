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

// DuckDuckGo Instant Answer API response structure
#[derive(Debug, Deserialize)]
struct DuckDuckGoResponse {
    #[serde(rename = "AbstractText")]
    abstract_text_plain: String,

    #[serde(rename = "AbstractSource")]
    abstract_source: String,

    #[serde(rename = "AbstractURL")]
    abstract_url: String,

    #[serde(rename = "RelatedTopics")]
    related_topics: Vec<RelatedTopic>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RelatedTopic {
    Topic {
        #[serde(rename = "Text")]
        text: String,
        #[serde(rename = "FirstURL")]
        first_url: String,
    },
    TopicGroup {
        #[serde(rename = "Topics")]
        topics: Vec<TopicItem>,
    },
}

#[derive(Debug, Deserialize)]
struct TopicItem {
    #[serde(rename = "Text")]
    text: String,
    #[serde(rename = "FirstURL")]
    first_url: String,
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

        // Use DuckDuckGo Instant Answer API (no API key required)
        let url = format!(
            "https://api.duckduckgo.com/?q={}&format=json&no_html=1&skip_disambig=1",
            urlencoding::encode(&input.query)
        );

        // Make HTTP request
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|e| ToolError::Execution(format!("Failed to create HTTP client: {}", e)))?;

        let response = client
            .get(&url)
            .send()
            .await
            .map_err(|e| ToolError::Execution(format!("Search request failed: {}", e)))?;

        if !response.status().is_success() {
            return Ok(ToolResult::error(format!(
                "Search request failed with status: {}",
                response.status()
            )));
        }

        let ddg_response: DuckDuckGoResponse = response
            .json()
            .await
            .map_err(|e| ToolError::Execution(format!("Failed to parse search results: {}", e)))?;

        // Build formatted output
        let mut output = String::new();
        output.push_str(&format!("🔍 Search results for: \"{}\"\n\n", input.query));

        // Add main abstract if available
        if !ddg_response.abstract_text_plain.is_empty() {
            output.push_str("📄 Summary:\n");
            output.push_str(&ddg_response.abstract_text_plain);
            output.push_str("\n\n");

            if !ddg_response.abstract_url.is_empty() {
                output.push_str(&format!(
                    "Source: {} - {}\n\n",
                    ddg_response.abstract_source, ddg_response.abstract_url
                ));
            }
        }

        // Add related topics
        let mut result_count = 0;
        if !ddg_response.related_topics.is_empty() {
            output.push_str("📌 Related Results:\n\n");

            for topic in ddg_response.related_topics {
                if result_count >= input.max_results {
                    break;
                }

                match topic {
                    RelatedTopic::Topic { text, first_url } => {
                        output.push_str(&format!("{}. {}\n", result_count + 1, text));
                        output.push_str(&format!("   🔗 {}\n\n", first_url));
                        result_count += 1;
                    }
                    RelatedTopic::TopicGroup { topics } => {
                        for topic_item in topics {
                            if result_count >= input.max_results {
                                break;
                            }
                            output.push_str(&format!(
                                "{}. {}\n",
                                result_count + 1,
                                topic_item.text
                            ));
                            output.push_str(&format!("   🔗 {}\n\n", topic_item.first_url));
                            result_count += 1;
                        }
                    }
                }
            }
        }

        if output.len() <= 100 {
            // Very little content returned
            output.clear();
            output.push_str(&format!("🔍 Search results for: \"{}\"\n\n", input.query));
            output.push_str("ℹ️  No detailed results found. Try:\n");
            output.push_str("  • Rephrasing your query\n");
            output.push_str("  • Using more specific keywords\n");
            output.push_str("  • Searching for a different topic\n");
        }

        Ok(ToolResult::success(output))
    }
}
