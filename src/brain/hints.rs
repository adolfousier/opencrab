//! Harness-driven brain file hints for tool calls and errors.
//!
//! The model doesn't need to remember to load TOOLS.md — the harness searches
//! brain files (TOOLS.md, AGENTS.md, CODE.md, SOUL.md, SECURITY.md) via the
//! existing FTS5 index when:
//!   • a tool call fails (error path injection)
//!   • a tool is discovered via tool_search (discovery-time injection)
//!
//! Uses the qmd FTS5 engine already loaded for memory search. Sub-millisecond.
//! Content-based — no heading format required, works with any TOOLS.md layout.

use tokio::sync::OnceCell;

use crate::memory::{self, MemoryResult};

/// Max snippets to inject per hint. 2 is enough to surface the gotcha without
/// flooding the tool result with prose.
const MAX_SNIPPETS: usize = 2;
/// Max chars per snippet (truncated at word boundary).
const MAX_SNIPPET_CHARS: usize = 400;
/// Max total chars for the hint block. Keeps tool results lean.
const MAX_HINT_CHARS: usize = 900;

/// Global guard: if the FTS index isn't ready (cold start, no brain files),
/// don't block the tool loop retrying. Set after first successful search.
static INDEX_READY: OnceCell<bool> = OnceCell::const_new();

/// Get relevant brain-file snippets for a tool name and optional error context.
///
/// Returns `None` when:
///   • the FTS store isn't available
///   • no brain file matches the query
///   • the index is empty (no brain files on disk)
pub async fn hints_for_tool(tool_name: &str, error_context: Option<&str>) -> Option<String> {
    // Skip if we've already established the index is empty/unavailable.
    if let Some(false) = INDEX_READY.get() {
        return None;
    }

    let store = match memory::get_store() {
        Ok(s) => s,
        Err(_) => {
            let _ = INDEX_READY.set(false);
            return None;
        }
    };

    let query = match error_context {
        Some(ctx) if !ctx.is_empty() => format!("{tool_name} {ctx}"),
        _ => tool_name.to_string(),
    };

    let results = match memory::search_brain(store, &query, MAX_SNIPPETS + 1).await {
        Ok(r) if !r.is_empty() => r,
        Ok(_) => {
            let _ = INDEX_READY.set(true); // index exists, just no hits
            return None;
        }
        Err(_) => {
            let _ = INDEX_READY.set(false);
            return None;
        }
    };

    let _ = INDEX_READY.set(true);

    let formatted = format_hints(&results);
    if formatted.is_empty() {
        None
    } else {
        Some(formatted)
    }
}

/// Format a short hint block from search results.
fn format_hints(results: &[MemoryResult]) -> String {
    let mut out = String::new();
    let mut total = 0;

    for hit in results.iter().take(MAX_SNIPPETS) {
        // Extract the file name from the full path for compact display.
        let source = std::path::Path::new(&hit.path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| hit.path.clone());

        let snippet = truncate_at_word(&hit.snippet, MAX_SNIPPET_CHARS);
        let block = format!("\n• **{source}** — {snippet}\n");
        if total + block.len() > MAX_HINT_CHARS {
            break;
        }
        out.push_str(&block);
        total += block.len();
    }

    if out.is_empty() {
        String::new()
    } else {
        format!("\n\n─── relevant notes ───{out}")
    }
}

/// Truncate `s` at the last word boundary before `max_chars`.
fn truncate_at_word(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        return s.to_string();
    }
    // Find the last whitespace before max_chars.
    let cut = s
        .char_indices()
        .take_while(|(i, _)| *i < max_chars)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(max_chars);
    let truncated = &s[..cut];
    // Trim to last whitespace for clean break.
    match truncated.rfind(char::is_whitespace) {
        Some(pos) if pos > cut / 2 => format!("{}…", &truncated[..pos]),
        _ => format!("{truncated}…"),
    }
}

/// Ensure brain files are indexed. Call at session start; idempotent.
pub async fn ensure_indexed() {
    let store = match memory::get_store() {
        Ok(s) => s,
        Err(_) => return,
    };
    if let Err(e) = memory::reindex(store).await {
        tracing::warn!("Brain file reindex failed: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_at_word_respects_boundary() {
        let s = "hello world foo bar baz qux quux";
        let t = truncate_at_word(s, 15);
        assert!(t.len() <= 16); // 15 + ellipsis
        assert!(t.ends_with('…'));
    }

    #[test]
    fn truncate_short_string_unchanged() {
        let s = "short";
        assert_eq!(truncate_at_word(s, 100), "short");
    }

    #[test]
    fn format_hints_empty() {
        assert_eq!(format_hints(&[]), "");
    }
}
