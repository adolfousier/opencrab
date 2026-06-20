//! Periodic brain-file deduplication scanner.
//!
//! Reads all brain files (SOUL.md, AGENTS.md, MEMORY.md, etc.) and
//! identifies exact duplicate lines or near-duplicate blocks. Results
//! are converted into `BrainDedupProposal` entries for Mission Control
//! review.
//!
//! The scanner is conservative: it only flags duplicates that are
//! clearly redundant (exact line matches, repeated blocks) and skips
//! structural markdown (headings, blank lines, separators). This
//! avoids false positives on intentional repetition (e.g., numbered
//! lists with similar prefixes).

use std::collections::HashMap;
use std::path::Path;

use crate::brain::rsi_proposals::ProposedBrainDedup;

/// Core brain files to scan (both CORE and CONTEXTUAL).
const BRAIN_FILES_TO_SCAN: &[&str] = &[
    "SOUL.md",
    "USER.md",
    "AGENTS.md",
    "CODE.md",
    "TOOLS.md",
    "SECURITY.md",
    "MEMORY.md",
    "BOOT.md",
    "HEARTBEAT.md",
];

/// Minimum line length to consider for dedup (skip short structural lines).
const MIN_LINE_LEN: usize = 10;

/// Minimum occurrences to flag as duplicate.
const MIN_DUPLICATE_COUNT: usize = 2;

/// Purpose-order ranking for canonical-file selection. Lower rank wins.
/// Issue #164 fix 3: identity-shaping files (SOUL, then AGENTS, TOOLS,
/// CODE, SECURITY, MEMORY, USER) outrank everything else, regardless of
/// alphabetical position. Unknown files fall to the bottom and tie-break
/// alphabetically via the caller's `.then(...)`.
pub(crate) fn canonical_file_rank(filename: &str) -> u8 {
    match filename {
        "SOUL.md" => 0,
        "AGENTS.md" => 1,
        "TOOLS.md" => 2,
        "CODE.md" => 3,
        "SECURITY.md" => 4,
        "MEMORY.md" => 5,
        "USER.md" => 6,
        _ => u8::MAX,
    }
}

/// Lines that look like markdown structure — skip these.
/// `pub(crate)` so the regression test file under `src/tests/` can
/// exercise it directly (memory rule forbids inline `#[cfg(test)] mod
/// tests` blocks; every test lives under `src/tests/` and is registered
/// in `mod.rs`).
pub(crate) fn is_structural_line(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return true;
    }
    if trimmed.len() < MIN_LINE_LEN {
        return true;
    }
    // Headings
    if trimmed.starts_with('#') {
        return true;
    }
    // Horizontal rules
    if trimmed
        .chars()
        .all(|c| c == '-' || c == '=' || c == '*' || c == '_')
    {
        return true;
    }
    // Table separators
    if trimmed.starts_with('|') && trimmed.ends_with('|') && trimmed.contains("---") {
        return true;
    }
    // Blockquotes with short content
    if trimmed.starts_with('>') && trimmed.len() < 20 {
        return true;
    }
    false
}

/// One cluster of duplicate content found by the scan.
#[derive(Debug, Clone)]
pub struct DuplicateCluster {
    /// The duplicated text (one instance).
    pub text: String,
    /// Where it appears: (filename, line_numbers 1-indexed).
    pub locations: Vec<(String, Vec<usize>)>,
    /// Total occurrence count across all locations.
    pub total_count: usize,
}

/// Scan all brain files for duplicate lines and blocks.
///
/// Returns a list of duplicate clusters, sorted by total_count descending.
/// Each cluster represents one piece of content that appears multiple times.
pub fn scan_brain_files(brain_path: &Path) -> Vec<DuplicateCluster> {
    let mut line_occurrences: HashMap<String, Vec<(String, usize)>> = HashMap::new();

    for filename in BRAIN_FILES_TO_SCAN {
        let file_path = brain_path.join(filename);
        if !file_path.exists() {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&file_path) else {
            continue;
        };

        for (line_idx, line) in content.lines().enumerate() {
            if is_structural_line(line) {
                continue;
            }
            let normalized = line.trim().to_string();
            if normalized.len() < MIN_LINE_LEN {
                continue;
            }
            line_occurrences
                .entry(normalized)
                .or_default()
                .push((filename.to_string(), line_idx + 1));
        }
    }

    // Group into clusters: only keep entries with >= MIN_DUPLICATE_COUNT
    let mut clusters: Vec<DuplicateCluster> = Vec::new();
    for (text, locations) in line_occurrences {
        let total_count: usize = locations.len();
        if total_count < MIN_DUPLICATE_COUNT {
            continue;
        }
        // Group by file
        let mut by_file: HashMap<String, Vec<usize>> = HashMap::new();
        for (file, line) in &locations {
            by_file.entry(file.clone()).or_default().push(*line);
        }
        let mut loc_vec: Vec<(String, Vec<usize>)> = by_file.into_iter().collect();
        // Purpose-ordered sort (issue #164 fix 3): pick the most semantically
        // authoritative file as canonical instead of the alphabetical winner.
        // Pre-fix, `AGENTS.md` beat `SOUL.md` purely by lexical order, so
        // identity-shaping lines kept getting proposed for removal from SOUL.
        loc_vec.sort_by(|a, b| {
            canonical_file_rank(&a.0)
                .cmp(&canonical_file_rank(&b.0))
                .then(a.0.cmp(&b.0))
        });

        clusters.push(DuplicateCluster {
            text,
            locations: loc_vec,
            total_count,
        });
    }

    // Sort by count descending, then by text for stability
    clusters.sort_by(|a, b| b.total_count.cmp(&a.total_count).then(a.text.cmp(&b.text)));
    clusters
}

/// Convert a duplicate cluster into a list of `ProposedBrainDedup`
/// payloads — one per non-canonical location.
///
/// Issue #164 fix 3: pre-fix this emitted at most ONE proposal per cluster
/// regardless of how many files held duplicates (comment in pre-fix code:
/// "For simplicity, target the second file in the list"). For a 5-file
/// duplicate, four proposals went silently uncreated and the duplicates
/// stayed on disk forever. Now we generate N-1 proposals — one per non-
/// canonical file — so the inbox accurately reflects the cleanup work.
pub fn cluster_to_proposals(cluster: &DuplicateCluster) -> Vec<ProposedBrainDedup> {
    if cluster.locations.is_empty() {
        return Vec::new();
    }

    let (canonical_file, canonical_lines) = &cluster.locations[0];
    let mut proposals = Vec::new();

    if cluster.locations.len() == 1 {
        // Same file, multiple occurrences — one proposal removing the
        // non-first occurrences within this file.
        if canonical_lines.len() <= 1 {
            return proposals;
        }
        let lines_to_remove = canonical_lines[1..].to_vec();
        if let Some(p) = build_proposal(
            cluster,
            canonical_file,
            canonical_lines[0],
            canonical_file,
            &lines_to_remove,
        ) {
            proposals.push(p);
        }
        return proposals;
    }

    // Multiple files — one proposal per non-canonical file. Each names
    // the canonical file/line as `duplicate_of` and lists the line range
    // to remove from the target.
    for (other_file, other_lines) in cluster.locations.iter().skip(1) {
        if let Some(p) = build_proposal(
            cluster,
            canonical_file,
            canonical_lines[0],
            other_file,
            other_lines,
        ) {
            proposals.push(p);
        }
    }
    proposals
}

/// Backwards-compatible single-proposal wrapper. Returns the first proposal
/// from `cluster_to_proposals` so existing call sites keep working while
/// we transition to N-1 semantics. New code should call `cluster_to_proposals`.
#[deprecated(note = "use cluster_to_proposals for N-1 per-file proposals")]
pub fn cluster_to_proposal(cluster: &DuplicateCluster) -> Option<ProposedBrainDedup> {
    cluster_to_proposals(cluster).into_iter().next()
}

fn build_proposal(
    cluster: &DuplicateCluster,
    canonical_file: &str,
    canonical_first_line: usize,
    target_file: &str,
    lines_to_remove: &[usize],
) -> Option<ProposedBrainDedup> {
    if lines_to_remove.is_empty() {
        return None;
    }
    let line_range = if lines_to_remove.len() == 1 {
        format!("{}", lines_to_remove[0])
    } else {
        format!(
            "{}-{}",
            lines_to_remove.iter().min().unwrap(),
            lines_to_remove.iter().max().unwrap()
        )
    };
    let duplicate_of = format!("{}:{}", canonical_file, canonical_first_line);
    Some(ProposedBrainDedup {
        target_file: target_file.to_string(),
        duplicate_text: cluster.text.clone(),
        line_range,
        duplicate_of,
        count: lines_to_remove.len(),
        warnings: Vec::new(),
    })
}

/// Run the full scan and return proposals ready for the inbox.
///
/// Issue #164 fix 3: now emits N-1 proposals per cluster (was 1) AND
/// runs a post-hoc stub-risk scan that annotates each proposal's
/// `warnings` field with the names of headers whose body region would
/// be emptied by the proposed removals.
pub fn generate_dedup_proposals(brain_path: &Path) -> Vec<(ProposedBrainDedup, String)> {
    let clusters = scan_brain_files(brain_path);
    let mut results = Vec::new();

    // Build a per-file map of all lines proposed for removal across all
    // clusters in this scan. The stub-risk check needs to consider the
    // CUMULATIVE removal set so it doesn't miss the case where two
    // clusters each remove half of a header's body lines.
    let mut planned_removals: HashMap<String, Vec<usize>> = HashMap::new();
    let mut staged: Vec<(ProposedBrainDedup, String)> = Vec::new();

    for cluster in &clusters {
        for proposal in cluster_to_proposals(cluster) {
            let rationale = format!(
                "Found '{}' appearing {} times across brain files. \
                 Keeping canonical copy at {}, removing duplicate(s).",
                &cluster.text[..cluster.text.len().min(80)],
                cluster.total_count,
                proposal.duplicate_of,
            );
            planned_removals
                .entry(proposal.target_file.clone())
                .or_default()
                .extend(parse_line_range(&proposal.line_range));
            staged.push((proposal, rationale));
        }
    }

    // Post-hoc stub-risk scan. For each affected file, re-read it and
    // compute which header bodies would be emptied by the planned
    // removals, then thread those warnings back into each proposal that
    // touches that file.
    let stub_risk_by_file: HashMap<String, Vec<String>> = planned_removals
        .iter()
        .map(|(filename, removed)| {
            let warnings = compute_stub_risk(brain_path, filename, removed);
            (filename.clone(), warnings)
        })
        .collect();

    for (mut proposal, rationale) in staged {
        if let Some(warnings) = stub_risk_by_file.get(&proposal.target_file)
            && !warnings.is_empty()
        {
            // Attribute every per-file warning to every proposal that
            // touches the file. A more precise per-proposal attribution
            // would need to re-simulate each removal individually; the
            // current shape errs on the side of surfacing the same
            // warning twice rather than missing it.
            proposal.warnings = warnings.clone();
        }
        results.push((proposal, rationale));
    }

    results
}

/// Parse a `line_range` field ("42" or "42-58") back into the explicit
/// list of line numbers in the range. The stub-risk scan needs the
/// individual numbers, not just the bounds.
fn parse_line_range(range: &str) -> Vec<usize> {
    if let Some((start, end)) = range.split_once('-') {
        let start = start.trim().parse::<usize>().ok();
        let end = end.trim().parse::<usize>().ok();
        match (start, end) {
            (Some(s), Some(e)) if e >= s => (s..=e).collect(),
            _ => Vec::new(),
        }
    } else if let Ok(n) = range.trim().parse::<usize>() {
        vec![n]
    } else {
        Vec::new()
    }
}

/// For a single brain file plus the set of line numbers proposed for
/// removal, return the list of header lines whose body region would be
/// empty after the removals. Empty Vec means no stub risk.
///
/// "Empty" matches the rules in `brain::filter::strip_empty_sections` —
/// blank, horizontal rule, table separator, short blockquote, HTML
/// comment. Real content (including TBD/TODO/WIP/placeholder markers)
/// keeps the section alive.
fn compute_stub_risk(brain_path: &Path, filename: &str, removed: &[usize]) -> Vec<String> {
    let file_path = brain_path.join(filename);
    let Ok(content) = std::fs::read_to_string(&file_path) else {
        return Vec::new();
    };
    let removed_set: std::collections::HashSet<usize> = removed.iter().copied().collect();

    // Build the post-removal view (1-indexed line filter), then run the
    // filter module's strip detector against it. Headers it identifies
    // as having empty bodies are the stub risks.
    let post: Vec<&str> = content
        .lines()
        .enumerate()
        .filter_map(|(i, l)| {
            if removed_set.contains(&(i + 1)) {
                None
            } else {
                Some(l)
            }
        })
        .collect();
    let post_str = post.join("\n");

    // Compute headers in original content. If any header was non-empty
    // before but its body is empty after, it's a stub-risk.
    let pre_headers = headers_with_empty_body(&content);
    let post_headers = headers_with_empty_body(&post_str);

    // Stub-risk set = post − pre (headers that newly became empty).
    let mut out: Vec<String> = post_headers
        .into_iter()
        .filter(|h| !pre_headers.contains(h))
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Headers whose body region is empty by the same definition as
/// `brain::filter::strip_empty_sections`. We delegate to the filter
/// module so the dedup proposal warnings and the read-time strip stay
/// in lockstep — what one calls "stub" the other must agree on.
fn headers_with_empty_body(content: &str) -> std::collections::HashSet<String> {
    let res = crate::brain::filter::strip_empty_sections(content);
    res.stripped_headers.into_iter().collect()
}

/// Run the scan and file proposals into the ProposalsStore.
///
/// Each duplicate cluster becomes one `BrainDedupProposal` in the
/// inbox. The proposer is set to "rsi-dedup-scan" so the user can
/// distinguish these from other RSI proposals. Returns the number
/// of proposals filed.
pub fn file_dedup_proposals(
    brain_path: &Path,
    store: &crate::brain::rsi_proposals::ProposalsStore,
) -> usize {
    let proposals = generate_dedup_proposals(brain_path);
    let mut count = 0;
    for (dedup, rationale) in proposals {
        match store.add_brain_dedup_proposal("rsi-dedup-scan", rationale, dedup) {
            Ok(_id) => count += 1,
            Err(e) => {
                tracing::warn!("Failed to file brain dedup proposal: {e}");
            }
        }
    }
    count
}

// Tests live under `src/tests/rsi_brain_dedup_test.rs` per project
// policy (no inline `#[cfg(test)] mod tests` blocks). Internal helpers
// like `is_structural_line` and `canonical_file_rank` are `pub(crate)`
// so the test file can reach them directly.
