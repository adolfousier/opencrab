//! Narrow a lexical document hit down to the chunk that actually matched
//! (#1000).
//!
//! `documents_fts` holds one row per DOCUMENT, so `search_fts` answers "this
//! file is relevant" and nothing more. After #998 the vector half ranks chunks,
//! which left the two halves of hybrid search describing different units: RRF
//! still produces an ordering, but the lexical side contributes a coarser
//! signal than it should, and a document whose relevant passage sits late in
//! the file is under-served by exactly the half that should find an exact term
//! match.
//!
//! This refines a document hit to its best chunk rather than building a second
//! FTS index. Two reasons:
//!
//! - The chunk boundaries already exist and are deterministic. `chunks_for` is
//!   the single place that decides them, so recomputing them from the body
//!   yields the same pieces the embedder used, and both halves agree without
//!   storing anything twice.
//! - `documents_fts` is kept in sync by triggers on `documents`, one FTS row
//!   per document. Writing chunk rows into it would double-store content and
//!   fight the trigger-managed rowids.
//!
//! Scoring reuses `section_rank`, so chunk-level lexical matching inherits the
//! stemming and diacritic folding rather than growing a second implementation
//! that drifts.

use crate::brain::brain_sections::Section;
use crate::brain::section_rank::Ranked;

/// The chunk of `body` that best matches `query`.
///
/// Returns the chunk text and its character offset, or `None` when the body is
/// empty or nothing scores above zero. `None` means the caller should fall back
/// to whatever it did before, not that the document is irrelevant: FTS already
/// judged relevance, this only decides WHERE.
pub fn best_chunk(body: &str, query: &str) -> Option<(usize, String)> {
    if body.trim().is_empty() {
        return None;
    }

    let chunks = super::embedding::chunks_for(body);
    if chunks.len() <= 1 {
        // Nothing to narrow: a single chunk IS the document.
        return None;
    }

    // Chunks carry no heading, so the whole chunk is the body. The heading
    // slot stays empty rather than inventing one, since a fabricated heading
    // would be scored as if the author had written it.
    let sections: Vec<Section> = chunks
        .iter()
        .map(|c| Section {
            heading: String::new(),
            body: c.text.clone(),
        })
        .collect();

    let ranked = Ranked::from_sections(sections);
    // No score floor: FTS has already decided this document is relevant, so the
    // job here is only to pick the best of its chunks. Applying a threshold
    // could reject every chunk and lose a hit the lexical half had earned.
    let matches = ranked.find_relevant(query, 1, usize::MAX, 0.0);
    let best = matches.sections.first()?;

    // Map the winning chunk back to its offset in the document.
    let pos = chunks
        .iter()
        .find(|c| c.text == best.body)
        .map(|c| c.pos)
        .unwrap_or(0);

    Some((pos, best.body.clone()))
}
