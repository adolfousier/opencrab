//! Glob Pattern Matching Tool
//!
//! Find files matching glob patterns.

use super::error::{Result, ToolError};
use super::r#trait::{Tool, ToolCapability, ToolExecutionContext, ToolResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

/// Glob pattern matching tool
pub struct GlobTool;

#[derive(Debug, Deserialize, Serialize)]
struct GlobInput {
    /// Glob pattern to match
    pattern: String,

    /// Base directory for search (defaults to working directory)
    #[serde(default)]
    base_dir: Option<String>,

    /// Maximum number of results to return
    #[serde(default)]
    limit: Option<usize>,

    /// Include hidden files
    #[serde(default)]
    include_hidden: bool,
}

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &str {
        "glob"
    }

    fn description(&self) -> &str {
        "Find files matching a glob pattern. Supports wildcards: * (any chars), ** (recursive directories), ? (single char), [abc] (char class)."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern (e.g., '**/*.rs', 'src/**/*.test.js', '*.{md,txt}')"
                },
                "base_dir": {
                    "type": "string",
                    "description": "Base directory for search (defaults to working directory)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of results to return",
                    "minimum": 1
                },
                "include_hidden": {
                    "type": "boolean",
                    "description": "Include hidden files (starting with .)",
                    "default": false
                }
            },
            "required": ["pattern"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadFiles]
    }

    fn requires_approval(&self) -> bool {
        false // Pattern matching is safe
    }

    fn validate_input(&self, input: &Value) -> Result<()> {
        let input: GlobInput = serde_json::from_value(input.clone())
            .map_err(|e| ToolError::InvalidInput(format!("Invalid input: {}", e)))?;

        if input.pattern.trim().is_empty() {
            return Err(ToolError::InvalidInput(
                "Pattern cannot be empty".to_string(),
            ));
        }

        Ok(())
    }

    async fn execute(&self, input: Value, context: &ToolExecutionContext) -> Result<ToolResult> {
        let input: GlobInput = serde_json::from_value(input)?;

        // Resolve base directory (tilde expansion + absolute/relative resolution).
        let base_dir = if let Some(ref dir) = input.base_dir {
            super::error::resolve_tool_path(dir, &context.working_dir())
        } else {
            context.working_dir()
        };

        if !base_dir.exists() {
            return Ok(ToolResult::error(format!(
                "Base directory does not exist: {}",
                base_dir.display()
            )));
        }

        // Compile the pattern once and match it against each walked path,
        // rather than letting the `glob` crate drive the filesystem walk. The
        // crate's walk follows symlinks (a loop hangs forever), has no time or
        // entry bound, and runs synchronously on the async executor — a `**`
        // from a large home dir once hung for 16+ minutes in production.
        let pattern = glob::Pattern::new(&input.pattern)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid glob pattern: {}", e)))?;

        let include_hidden = input.include_hidden;
        let limit = input.limit;
        let base = base_dir.clone();

        // Hard bounds so a pathological tree can't hang the agent.
        const MAX_ENTRIES: usize = 500_000;
        const WALK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

        // The walk is blocking — run it off the async executor.
        let walk = tokio::task::spawn_blocking(move || {
            use ignore::WalkBuilder;
            let mut builder = WalkBuilder::new(&base);
            builder
                // NEVER follow symlinks — this is the loop that hung for minutes.
                .follow_links(false)
                // Descend into dotdirs (the target may live under ~/.opencrabs),
                // but disable all gitignore handling so a `.gitignore` (e.g. the
                // `*` one in ~/.opencrabs) doesn't hide the very files we seek.
                .hidden(false)
                .git_ignore(false)
                .git_global(false)
                .git_exclude(false)
                .ignore(false)
                .parents(false)
                // Prune notorious heavyweight dirs that blow up walk time and are
                // never what a file-find wants.
                .filter_entry(|e| {
                    !matches!(
                        e.file_name().to_str(),
                        Some("node_modules" | ".git" | "target" | ".cache")
                    )
                });

            let mut matches: Vec<PathBuf> = Vec::new();
            let mut scanned = 0usize;
            let mut truncated = false;
            for dent in builder.build() {
                scanned += 1;
                if scanned > MAX_ENTRIES {
                    truncated = true;
                    break;
                }
                let dent = match dent {
                    Ok(d) => d,
                    Err(e) => {
                        tracing::warn!("glob: error reading entry: {}", e);
                        continue;
                    }
                };
                let path = dent.path();
                let rel = path.strip_prefix(&base).unwrap_or(path);
                if !pattern.matches_path(rel) {
                    continue;
                }
                // Preserve the original "skip dot-FILES unless include_hidden"
                // behaviour (filters the final component only, not parent dirs).
                if !include_hidden
                    && path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(|s| s.starts_with('.'))
                        .unwrap_or(false)
                {
                    continue;
                }
                matches.push(path.to_path_buf());
                if let Some(limit) = limit
                    && matches.len() >= limit
                {
                    break;
                }
            }
            (matches, truncated)
        });

        let (mut matches, truncated) = match tokio::time::timeout(WALK_TIMEOUT, walk).await {
            Ok(Ok(result)) => result,
            Ok(Err(e)) => {
                return Ok(ToolResult::error(format!("glob walk failed: {e}")));
            }
            Err(_) => {
                return Ok(ToolResult::error(format!(
                    "glob timed out after {}s scanning '{}'. The tree is too large — \
                     narrow the pattern or pass a more specific `base_dir`.",
                    WALK_TIMEOUT.as_secs(),
                    base_dir.display()
                )));
            }
        };

        if matches.is_empty() {
            return Ok(ToolResult::success(format!(
                "No files found matching pattern: {}",
                input.pattern
            )));
        }

        // Sort matches for consistent output
        matches.sort();

        // Format output
        let mut output = format!(
            "Found {} files matching '{}':\n\n",
            matches.len(),
            input.pattern
        );

        for path in &matches {
            // Make path relative to base_dir for cleaner output
            let display_path = path
                .strip_prefix(&base_dir)
                .unwrap_or(path)
                .display()
                .to_string();
            output.push_str(&format!("  {}\n", display_path));
        }

        if let Some(limit) = input.limit
            && matches.len() >= limit
        {
            output.push_str(&format!("\n(Limited to {} results)", limit));
        }

        if truncated {
            output.push_str(&format!(
                "\n(Stopped after scanning {MAX_ENTRIES} entries — tree too large; \
                 narrow the pattern or pass a more specific base_dir)"
            ));
        }

        Ok(ToolResult::success(output))
    }
}
