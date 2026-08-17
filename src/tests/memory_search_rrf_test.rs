//! RRF fusion of FTS and vector hits.
//!
//! Moved out of `src/memory/search.rs`: tests live under `src/tests/`,
//! never inline beside the logic they exercise (#1076).

use crate::memory::search::*;

fn hit(file: &str) -> (String, String, String, String) {
    (
        file.to_string(),
        file.to_string(),
        format!("title of {file}"),
        format!("body of {file}"),
    )
}

#[test]
fn rrf_prefers_docs_in_both_lists() {
    let fts = vec![hit("a.md"), hit("b.md")];
    let vec = vec![hit("b.md"), hit("c.md")];
    let fused = hybrid_search_rrf(fts, vec, 60);

    // b.md appears in both lists, so it outranks single-list hits.
    assert_eq!(fused[0].file, "b.md");
    assert!(fused[0].score > fused[1].score);
}

#[test]
fn rrf_keeps_bodies_stable() {
    let fused = hybrid_search_rrf(vec![hit("a.md")], vec![], 60);
    assert_eq!(fused.len(), 1);
    assert_eq!(fused[0].body, "body of a.md");
    // Rank-1 bonus applies.
    assert!(fused[0].score > 0.08);
}
