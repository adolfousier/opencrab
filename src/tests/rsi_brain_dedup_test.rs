//! Regression tests for the brain-file dedup scan and proposal flow.
//!
//! Covers: duplicate detection across files, near-duplicate detection,
//! periodicity gating, proposal format correctness, empty brain files
//! handled, files under min size skipped.

use crate::brain::dedup_scan::{
    DuplicateCluster, canonical_file_rank, cluster_to_proposals, file_dedup_proposals,
    generate_dedup_proposals, is_structural_line, scan_brain_files,
};
use crate::brain::rsi_proposals::ProposalsStore;
use std::fs;
use tempfile::TempDir;

fn write_brain_file(dir: &std::path::Path, name: &str, content: &str) {
    fs::write(dir.join(name), content).unwrap();
}

// --- Periodicity gating ---

#[test]
fn test_scan_does_not_fire_on_empty_brain_dir() {
    let dir = TempDir::new().unwrap();
    // No brain files at all
    let proposals = generate_dedup_proposals(dir.path());
    assert!(proposals.is_empty());
}

#[test]
fn test_scan_handles_missing_files_gracefully() {
    let dir = TempDir::new().unwrap();
    // Write only SOUL.md, all other brain files are missing
    write_brain_file(
        dir.path(),
        "SOUL.md",
        "# SOUL.md\n\nThis is unique content that appears once.\n",
    );
    let proposals = generate_dedup_proposals(dir.path());
    assert!(proposals.is_empty());
}

// --- Empty brain files handled ---

#[test]
fn test_empty_brain_file_produces_no_proposals() {
    let dir = TempDir::new().unwrap();
    write_brain_file(dir.path(), "SOUL.md", "");
    write_brain_file(dir.path(), "AGENTS.md", "");
    let proposals = generate_dedup_proposals(dir.path());
    assert!(proposals.is_empty());
}

#[test]
fn test_whitespace_only_brain_file_produces_no_proposals() {
    let dir = TempDir::new().unwrap();
    write_brain_file(dir.path(), "SOUL.md", "\n\n\n   \n\n");
    let proposals = generate_dedup_proposals(dir.path());
    assert!(proposals.is_empty());
}

// --- Files under min size skipped ---

#[test]
fn test_short_lines_are_skipped() {
    let dir = TempDir::new().unwrap();
    // Lines shorter than MIN_LINE_LEN (10 chars) should be ignored
    let content = "# SOUL.md\n\nshort\n\nshort\n\nshort\n";
    write_brain_file(dir.path(), "SOUL.md", content);
    let clusters = scan_brain_files(dir.path());
    assert!(clusters.is_empty(), "Short lines should be skipped");
}

#[test]
fn test_only_heading_lines_produces_no_clusters() {
    let dir = TempDir::new().unwrap();
    let content = "# Heading One\n\n# Heading Two\n\n# Heading Three\n";
    write_brain_file(dir.path(), "SOUL.md", content);
    let clusters = scan_brain_files(dir.path());
    assert!(clusters.is_empty());
}

// --- Duplicate detection across files ---

#[test]
fn test_duplicate_across_three_files() {
    let dir = TempDir::new().unwrap();
    let shared = "This rule about git commits is shared across multiple brain files for emphasis.";
    write_brain_file(
        dir.path(),
        "SOUL.md",
        &format!("# SOUL\n\n{}\n\nUnique soul stuff.\n", shared),
    );
    write_brain_file(
        dir.path(),
        "AGENTS.md",
        &format!("# AGENTS\n\n{}\n\nUnique agents stuff.\n", shared),
    );
    write_brain_file(
        dir.path(),
        "MEMORY.md",
        &format!("# MEMORY\n\n{}\n\nUnique memory stuff.\n", shared),
    );

    let clusters = scan_brain_files(dir.path());
    assert_eq!(clusters.len(), 1);
    assert_eq!(clusters[0].total_count, 3);
    assert_eq!(clusters[0].locations.len(), 3);
}

// --- Proposal format correctness ---

#[test]
fn test_proposal_has_correct_fields() {
    let dir = TempDir::new().unwrap();
    let repeated = "Always run cargo clippy and cargo fmt before every commit without exception.";
    let content = format!(
        "# SOUL.md\n\n{}\n\nUnique line one.\n\n{}\n\nUnique line two.\n",
        repeated, repeated
    );
    write_brain_file(dir.path(), "SOUL.md", &content);

    let proposals = generate_dedup_proposals(dir.path());
    assert_eq!(proposals.len(), 1);

    let (proposal, rationale) = &proposals[0];
    assert_eq!(proposal.target_file, "SOUL.md");
    assert!(!proposal.duplicate_text.is_empty());
    assert!(!proposal.line_range.is_empty());
    assert!(proposal.duplicate_of.starts_with("SOUL.md:"));
    assert!(proposal.count >= 1);
    assert!(!rationale.is_empty());
}

#[test]
fn test_proposal_count_reflects_removable_duplicates() {
    let dir = TempDir::new().unwrap();
    let repeated = "Never use em dashes in any output or code comments at all.";
    // 4 occurrences — keep 1 canonical, remove 3
    let content = format!(
        "# SOUL.md\n\n{}\n\nA\n\n{}\n\nB\n\n{}\n\nC\n\n{}\n",
        repeated, repeated, repeated, repeated
    );
    write_brain_file(dir.path(), "SOUL.md", &content);

    let proposals = generate_dedup_proposals(dir.path());
    assert_eq!(proposals.len(), 1);
    let (proposal, _) = &proposals[0];
    assert_eq!(
        proposal.count, 3,
        "Should count 3 removable duplicates (4 total - 1 canonical)"
    );
}

// --- ProposalsStore integration ---

#[test]
fn test_filed_proposals_appear_in_store() {
    let brain_dir = TempDir::new().unwrap();
    let rsi_dir = TempDir::new().unwrap();

    let repeated = "Keep responses under 3 sentences when possible in all contexts.";
    let content = format!(
        "# SOUL.md\n\n{}\n\nUnique.\n\n{}\n\nMore unique.\n",
        repeated, repeated
    );
    write_brain_file(brain_dir.path(), "SOUL.md", &content);

    let store = ProposalsStore::with_dir(rsi_dir.path().to_path_buf());
    let count = file_dedup_proposals(brain_dir.path(), &store);

    assert!(count > 0);
    let pending = store.list_brain_dedup_proposals();
    assert_eq!(pending.len(), count);

    // Verify proposal format in store
    let p = &pending[0];
    assert!(p.id.starts_with("prop_dedup_"));
    assert_eq!(p.proposer, "rsi-dedup-scan");
    assert!(!p.rationale.is_empty());
    assert_eq!(p.dedup.target_file, "SOUL.md");
}

#[test]
fn test_repeated_scan_dedups_proposals_in_store() {
    let brain_dir = TempDir::new().unwrap();
    let rsi_dir = TempDir::new().unwrap();

    let repeated = "Never push tags or releases without explicit user approval at all.";
    let content = format!(
        "# SOUL.md\n\n{}\n\nUnique.\n\n{}\n\nMore unique.\n",
        repeated, repeated
    );
    write_brain_file(brain_dir.path(), "SOUL.md", &content);

    let store = ProposalsStore::with_dir(rsi_dir.path().to_path_buf());

    // First scan
    let count1 = file_dedup_proposals(brain_dir.path(), &store);
    assert!(count1 > 0);

    // Second scan — should NOT create duplicate proposals (dedup by target_file + duplicate_text)
    let count2 = file_dedup_proposals(brain_dir.path(), &store);
    assert_eq!(
        count2, count1,
        "Second scan should supersede, not duplicate"
    );

    let pending = store.list_brain_dedup_proposals();
    assert_eq!(
        pending.len(),
        count1,
        "Store should have exactly count1 proposals"
    );
}

// --- cluster_to_proposals edge cases ---

#[test]
fn test_cluster_with_single_occurrence_returns_empty() {
    let cluster = DuplicateCluster {
        text: "Unique line that only appears once in the whole codebase.".to_string(),
        locations: vec![("SOUL.md".to_string(), vec![42])],
        total_count: 1,
    };
    let proposals = cluster_to_proposals(&cluster);
    assert!(
        proposals.is_empty(),
        "Single occurrence should not generate any proposals"
    );
}

#[test]
fn test_cluster_with_empty_locations_returns_empty() {
    let cluster = DuplicateCluster {
        text: "Some text here that is long enough to matter for scanning.".to_string(),
        locations: vec![],
        total_count: 0,
    };
    let proposals = cluster_to_proposals(&cluster);
    assert!(proposals.is_empty());
}

// --- Cross-file canonical selection (purpose-ordered, issue #164 fix 3) ---

#[test]
fn test_canonical_uses_purpose_order_not_alphabetical() {
    let dir = TempDir::new().unwrap();
    let shared = "This rule is shared between AGENTS and SOUL files for testing purposes.";
    write_brain_file(
        dir.path(),
        "SOUL.md",
        &format!("# SOUL\n\n{}\n\nSoul unique.\n", shared),
    );
    write_brain_file(
        dir.path(),
        "AGENTS.md",
        &format!("# AGENTS\n\n{}\n\nAgents unique.\n", shared),
    );

    let proposals = generate_dedup_proposals(dir.path());
    assert_eq!(proposals.len(), 1);
    let (proposal, _) = &proposals[0];
    // AGENTS.md comes before SOUL.md alphabetically, so AGENTS.md is canonical
    // and SOUL.md is the target for removal
    // Pre-fix this asserted SOUL.md as the target (alphabetical winner).
    // Post-fix, SOUL.md outranks AGENTS.md by purpose order, so AGENTS.md
    // becomes the target and SOUL.md the canonical authority.
    assert_eq!(
        proposal.target_file, "AGENTS.md",
        "with purpose order (SOUL > AGENTS), AGENTS.md must be the removal target"
    );
    assert!(
        proposal.duplicate_of.starts_with("SOUL.md:"),
        "duplicate_of must point at SOUL.md as the canonical source"
    );
}

// --- Tests moved from inline `mod tests` in dedup_scan.rs ---

#[test]
fn test_structural_line_detection() {
    assert!(is_structural_line(""));
    assert!(is_structural_line("# Heading"));
    assert!(is_structural_line("---"));
    assert!(is_structural_line("| --- | --- |"));
    assert!(!is_structural_line(
        "This is a real line with actual content that should be scanned"
    ));
}

#[test]
fn test_scan_finds_duplicates_in_same_file() {
    let dir = TempDir::new().unwrap();
    let content = "# SOUL.md\n\n\
                    Keep responses under 3 sentences when possible.\n\n\
                    Some other content here that is unique.\n\n\
                    Keep responses under 3 sentences when possible.\n\n\
                    More unique content here.\n";
    write_brain_file(dir.path(), "SOUL.md", content);

    let clusters = scan_brain_files(dir.path());
    assert!(!clusters.is_empty(), "Should find duplicate line");

    let dup = &clusters[0];
    assert_eq!(dup.total_count, 2);
    assert!(dup.text.contains("Keep responses under 3 sentences"));
}

#[test]
fn test_scan_finds_duplicates_across_files() {
    let dir = TempDir::new().unwrap();
    let shared_line =
        "Never push tags or releases without EXPLICIT user approval. This is critical.";
    write_brain_file(
        dir.path(),
        "SOUL.md",
        &format!("# SOUL.md\n\n{}\n\nOther soul content.\n", shared_line),
    );
    write_brain_file(
        dir.path(),
        "AGENTS.md",
        &format!("# AGENTS.md\n\n{}\n\nOther agents content.\n", shared_line),
    );

    let clusters = scan_brain_files(dir.path());
    assert!(!clusters.is_empty(), "Should find cross-file duplicate");

    let dup = &clusters[0];
    assert_eq!(dup.total_count, 2);
    assert_eq!(dup.locations.len(), 2);
}

#[test]
fn test_scan_skips_structural_lines() {
    let dir = TempDir::new().unwrap();
    let content = "# SOUL.md\n\n# Heading\n\n# Heading\n\n---\n\n---\n";
    write_brain_file(dir.path(), "SOUL.md", content);

    let clusters = scan_brain_files(dir.path());
    assert!(
        clusters.is_empty(),
        "Should not flag headings or separators as duplicates"
    );
}

#[test]
fn test_cluster_to_proposals_same_file() {
    let cluster = DuplicateCluster {
        text: "Duplicate line content here that is long enough".to_string(),
        locations: vec![("SOUL.md".to_string(), vec![5, 10, 15])],
        total_count: 3,
    };

    let proposals = cluster_to_proposals(&cluster);
    assert_eq!(proposals.len(), 1, "same-file dedup is one proposal");
    let proposal = &proposals[0];
    assert_eq!(proposal.target_file, "SOUL.md");
    assert_eq!(proposal.duplicate_of, "SOUL.md:5");
    assert_eq!(proposal.count, 2);
    assert!(proposal.line_range.contains("10"));
}

#[test]
fn test_generate_dedup_proposals_end_to_end() {
    let dir = TempDir::new().unwrap();
    let repeated = "Keep responses under 3 sentences when possible.";
    let content = format!(
        "# SOUL.md\n\n{}\n\nUnique content.\n\n{}\n\nMore unique.\n\n{}\n",
        repeated, repeated, repeated
    );
    write_brain_file(dir.path(), "SOUL.md", &content);

    let proposals = generate_dedup_proposals(dir.path());
    assert!(
        !proposals.is_empty(),
        "Should generate at least one proposal"
    );

    let (proposal, rationale) = &proposals[0];
    assert_eq!(proposal.target_file, "SOUL.md");
    assert!(rationale.contains("3 times"));
}

#[test]
fn test_no_duplicates_means_no_proposals() {
    let dir = TempDir::new().unwrap();
    let content = "# SOUL.md\n\nEvery line here is unique.\n\nNo repetition at all.\n\nCompletely distinct content.\n";
    write_brain_file(dir.path(), "SOUL.md", content);

    let proposals = generate_dedup_proposals(dir.path());
    assert!(
        proposals.is_empty(),
        "Should not generate proposals when no duplicates exist"
    );
}

#[test]
fn test_file_dedup_proposals_into_store() {
    let brain_dir = TempDir::new().unwrap();
    let rsi_dir = TempDir::new().unwrap();
    let repeated = "Keep responses under 3 sentences when possible.";
    let content = format!(
        "# SOUL.md\n\n{}\n\nUnique content.\n\n{}\n\nMore unique.\n\n{}\n",
        repeated, repeated, repeated
    );
    write_brain_file(brain_dir.path(), "SOUL.md", &content);

    let store = ProposalsStore::with_dir(rsi_dir.path().to_path_buf());
    let count = file_dedup_proposals(brain_dir.path(), &store);

    assert!(count > 0, "Should file at least one proposal");
    let pending = store.list_brain_dedup_proposals();
    assert_eq!(pending.len(), count);
    assert_eq!(pending[0].proposer, "rsi-dedup-scan");
    assert_eq!(pending[0].dedup.target_file, "SOUL.md");
}

// --- Issue #164 fix 3: purpose-order + N-1 + stub warnings ---

#[test]
fn canonical_file_rank_orders_soul_first() {
    assert!(canonical_file_rank("SOUL.md") < canonical_file_rank("AGENTS.md"));
    assert!(canonical_file_rank("AGENTS.md") < canonical_file_rank("TOOLS.md"));
    assert!(canonical_file_rank("TOOLS.md") < canonical_file_rank("CODE.md"));
    assert!(canonical_file_rank("CODE.md") < canonical_file_rank("SECURITY.md"));
    assert!(canonical_file_rank("SECURITY.md") < canonical_file_rank("MEMORY.md"));
    assert!(canonical_file_rank("MEMORY.md") < canonical_file_rank("USER.md"));
    // Any non-core file (a user's own .md) falls to the bottom of the order.
    assert!(canonical_file_rank("USER.md") < canonical_file_rank("some-user-file.md"));
}

#[test]
fn n_minus_one_proposals_for_three_file_cluster() {
    // Pre-fix: a 3-file duplicate produced exactly ONE proposal because
    // cluster_to_proposal hardcoded `cluster.locations[1]`. Post-fix it
    // produces N-1 = 2 proposals — one per non-canonical file.
    let cluster = DuplicateCluster {
        text: "Shared rule across three brain files for testing N-1 emission.".to_string(),
        locations: vec![
            ("SOUL.md".to_string(), vec![10]),
            ("AGENTS.md".to_string(), vec![20]),
            ("MEMORY.md".to_string(), vec![30]),
        ],
        total_count: 3,
    };
    let proposals = cluster_to_proposals(&cluster);
    assert_eq!(
        proposals.len(),
        2,
        "3-file cluster must produce N-1 = 2 proposals, not 1; got {:?}",
        proposals
    );
    // The canonical (SOUL.md by purpose order — already sorted upstream
    // by scan_brain_files) must NOT appear as a target.
    let targets: Vec<&str> = proposals.iter().map(|p| p.target_file.as_str()).collect();
    assert!(
        !targets.contains(&"SOUL.md"),
        "canonical SOUL.md must not be a removal target; got targets={:?}",
        targets
    );
    assert!(targets.contains(&"AGENTS.md"));
    assert!(targets.contains(&"MEMORY.md"));
    // Every proposal cites the canonical as duplicate_of.
    for p in &proposals {
        assert!(
            p.duplicate_of.starts_with("SOUL.md:"),
            "every proposal must cite SOUL.md as canonical source; got {}",
            p.duplicate_of
        );
    }
}

#[test]
fn five_file_cluster_emits_four_proposals() {
    let cluster = DuplicateCluster {
        text: "Wide-spread rule duplicated across five brain files for sanity.".to_string(),
        locations: vec![
            ("SOUL.md".to_string(), vec![1]),
            ("AGENTS.md".to_string(), vec![2]),
            ("TOOLS.md".to_string(), vec![3]),
            ("MEMORY.md".to_string(), vec![4]),
            ("USER.md".to_string(), vec![5]),
        ],
        total_count: 5,
    };
    let proposals = cluster_to_proposals(&cluster);
    assert_eq!(
        proposals.len(),
        4,
        "5-file cluster must produce N-1 = 4 proposals"
    );
}

#[test]
fn stub_warning_surfaces_when_removal_would_empty_a_section() {
    let dir = TempDir::new().unwrap();
    // SOUL.md has the canonical line in a section with its own real
    // content. AGENTS.md has the same line as the SOLE body content of
    // a section — removing it would leave the header with an empty body
    // (a stub-risk that the load-time strip would later catch).
    let repeated = "This rule is the sole body line for the stub-risk header here.";
    write_brain_file(
        dir.path(),
        "SOUL.md",
        &format!(
            "# SOUL.md\n\n## Real Section\n\nReal body content here that survives.\n\n{}\n\nMore real content here.\n",
            repeated
        ),
    );
    write_brain_file(
        dir.path(),
        "AGENTS.md",
        &format!("# AGENTS.md\n\n## Stub Risk\n\n{}\n", repeated),
    );

    let proposals = generate_dedup_proposals(dir.path());
    assert!(
        !proposals.is_empty(),
        "Should produce at least one proposal for the cross-file duplicate"
    );
    let agents_proposal = proposals
        .iter()
        .find(|(p, _)| p.target_file == "AGENTS.md")
        .map(|(p, _)| p)
        .expect("expected a proposal targeting AGENTS.md");
    assert!(
        !agents_proposal.warnings.is_empty(),
        "removing the sole body line under `## Stub Risk` must surface a stub warning; \
         got warnings={:?}",
        agents_proposal.warnings
    );
    assert!(
        agents_proposal
            .warnings
            .iter()
            .any(|w| w.contains("Stub Risk")),
        "warning text must name the at-risk header; got warnings={:?}",
        agents_proposal.warnings
    );
}

#[test]
fn no_stub_warning_when_section_has_other_content() {
    let dir = TempDir::new().unwrap();
    let repeated = "Some shared content that is reasonably long for the dedup scanner.";
    // Both files have the same line, but in BOTH the section has other
    // body content that survives removal — no stub risk.
    write_brain_file(
        dir.path(),
        "SOUL.md",
        &format!(
            "# SOUL.md\n\n## Section\n\nOther body here.\n\n{}\n\nMore body.\n",
            repeated
        ),
    );
    write_brain_file(
        dir.path(),
        "AGENTS.md",
        &format!(
            "# AGENTS.md\n\n## Section\n\nAgents other body.\n\n{}\n\nAgents more body.\n",
            repeated
        ),
    );

    let proposals = generate_dedup_proposals(dir.path());
    let agents_proposal = proposals
        .iter()
        .find(|(p, _)| p.target_file == "AGENTS.md")
        .map(|(p, _)| p)
        .expect("expected a proposal targeting AGENTS.md");
    assert!(
        agents_proposal.warnings.is_empty(),
        "no stub-risk expected when the section has other surviving content; \
         got warnings={:?}",
        agents_proposal.warnings
    );
}
