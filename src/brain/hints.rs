//! Harness-driven brain-file hints (#767).
//!
//! TOOLS.md and friends are effectively dead weight: the model rarely loads
//! them proactively, so when a tool call misses (unknown name, bad params,
//! execution error) the error string carries zero guidance. This module fixes
//! that at the harness layer instead of relying on the model to remember a
//! rule from twenty turns ago.
//!
//! When a tool miss happens, the harness searches the brain files over the
//! EXISTING FTS5 index (`memory::search_brain`) and injects the relevant
//! snippets inline into the tool result. The model reads the guidance as part
//! of the result it already gets: no new rule to remember, no extra
//! `load_brain_file` round-trip.
//!
//! FTS-only on purpose. Hints ride the critical path of every tool miss, so
//! they must be sub-millisecond; we skip vector embeddings and RRF entirely.

use crate::memory::{self, MemoryResult};
use std::sync::atomic::{AtomicBool, Ordering};

/// Max snippets injected per hint block. Keeps tool results lean.
const MAX_SNIPPETS: usize = 2;
/// Per-snippet character cap. Defensive re-cap: `search_brain` already
/// extracts ~200-char snippets, so this rarely bites.
const SNIPPET_CHARS: usize = 500;
/// Hard cap on the whole hint block so a verbose brain file cannot bloat a
/// tool result.
const MAX_HINT_CHARS: usize = 900;
/// Minimum BM25 rank to trust a match. Below this a hit is noise.
const MIN_RANK: f64 = 0.1;

/// Guard: once we learn the FTS store is unavailable, stop retrying on every
/// tool miss. Set once, read many, no lock on the hot path.
static STORE_UNAVAILABLE: AtomicBool = AtomicBool::new(false);

/// Search the brain files for guidance relevant to a tool miss and format it
/// as a hint block, or return `None` when nothing relevant was found.
///
/// `query` is free text the caller builds from the miss context, e.g.
/// `"telegram_send tool not found"` or a `tool_search` query. The brain files
/// are indexed at startup, so this is a pure FTS read.
pub async fn hints_for(query: &str) -> Option<String> {
    if STORE_UNAVAILABLE.load(Ordering::Relaxed) {
        return None;
    }
    let store = match memory::get_store() {
        Ok(s) => s,
        Err(_) => {
            STORE_UNAVAILABLE.store(true, Ordering::Relaxed);
            return None;
        }
    };
    // Over-fetch then filter: ask for more than we show so the rank filter
    // still leaves us with the best MAX_SNIPPETS matches.
    let results = match memory::search_brain(store, query, MAX_SNIPPETS * 3).await {
        Ok(r) => r,
        Err(_) => return None,
    };
    format_hints(&results)
}

/// Format FTS results into a compact hint block.
///
/// Returns `None` when no result clears `MIN_RANK`. Caps the snippet count at
/// `MAX_SNIPPETS`, each snippet at `SNIPPET_CHARS`, and the whole block at
/// `MAX_HINT_CHARS`.
fn format_hints(results: &[MemoryResult]) -> Option<String> {
    let mut block = String::from("\n\n─── relevant brain notes ───\n");
    let mut count = 0usize;
    for r in results {
        if count >= MAX_SNIPPETS {
            break;
        }
        if r.rank < MIN_RANK {
            continue;
        }
        let name = std::path::Path::new(&r.path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| r.path.clone());
        let snippet: String = r.snippet.chars().take(SNIPPET_CHARS).collect();
        block.push_str(&format!("• **{name}**: {snippet}\n"));
        count += 1;
    }
    if count == 0 {
        return None;
    }
    if block.chars().count() > MAX_HINT_CHARS {
        let mut capped: String = block.chars().take(MAX_HINT_CHARS).collect();
        capped.push('…');
        return Some(capped);
    }
    Some(block)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(path: &str, snippet: &str, rank: f64) -> MemoryResult {
        MemoryResult {
            path: path.to_string(),
            snippet: snippet.to_string(),
            rank,
        }
    }

    #[test]
    fn empty_results_yield_none() {
        assert_eq!(format_hints(&[]), None);
    }

    #[test]
    fn all_below_threshold_yield_none() {
        let results = vec![
            result("TOOLS.md", "telegram_send routing", 0.05),
            result("AGENTS.md", "github rules", 0.01),
        ];
        assert_eq!(format_hints(&results), None);
    }

    #[test]
    fn relevant_results_yield_block() {
        let results = vec![result("TOOLS.md", "telegram_send is canonical", 0.9)];
        let block = format_hints(&results).expect("expected a hint block");
        assert!(block.contains("relevant brain notes"));
        assert!(block.contains("TOOLS.md"));
        assert!(block.contains("telegram_send is canonical"));
    }

    #[test]
    fn low_rank_results_are_filtered() {
        let results = vec![
            result("TOOLS.md", "good match", 0.8),
            result("AGENTS.md", "noise", 0.02),
        ];
        let block = format_hints(&results).expect("expected a hint block");
        assert!(block.contains("TOOLS.md"));
        assert!(!block.contains("AGENTS.md"));
    }

    #[test]
    fn caps_at_max_snippets() {
        let results = vec![
            result("TOOLS.md", "one", 0.9),
            result("AGENTS.md", "two", 0.8),
            result("CODE.md", "three", 0.7),
        ];
        let block = format_hints(&results).expect("expected a hint block");
        assert!(block.contains("TOOLS.md"));
        assert!(block.contains("AGENTS.md"));
        assert!(!block.contains("CODE.md"));
    }

    #[test]
    fn truncates_long_snippets() {
        let long = "y".repeat(600);
        let results = vec![result("TOOLS.md", &long, 0.9)];
        let block = format_hints(&results).expect("expected a hint block");
        let ys: usize = block.chars().filter(|c| *c == 'y').count();
        assert_eq!(ys, SNIPPET_CHARS);
    }

    #[test]
    fn caps_whole_block() {
        let results = vec![
            result("TOOLS.md", &"a".repeat(500), 0.9),
            result("AGENTS.md", &"b".repeat(500), 0.8),
        ];
        let block = format_hints(&results).expect("expected a hint block");
        // Whole-block cap: MAX_HINT_CHARS plus the trailing ellipsis.
        assert!(block.chars().count() <= MAX_HINT_CHARS + 1);
    }

    #[test]
    fn uses_file_name_not_full_path() {
        let results = vec![result("/home/u/.opencrabs/TOOLS.md", "routing", 0.9)];
        let block = format_hints(&results).expect("expected a hint block");
        assert!(block.contains("TOOLS.md"));
        assert!(!block.contains("/home/u"));
    }
}
