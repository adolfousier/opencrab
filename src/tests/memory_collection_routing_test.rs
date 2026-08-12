//! An incremental index must file a document in the same collection the full
//! reindex would (#1018 follow-up).
//!
//! `reindex` has always split them: daily notes into the memory collection,
//! brain files into the brain collection. `index_file` — the incremental path
//! used by the write hook and the freshness check — hardcoded memory. A brain
//! file written mid-session therefore landed in the wrong collection, where
//! `search_brain` never looks, while the brain row it should have updated went
//! untouched.
//!
//! Caught on a live index: `AGENTS.md` held two rows, the brain one live and a
//! memory-collection copy left inactive by the next reindex. The incremental
//! index had been doing nothing for brain search while appearing to succeed.

use crate::memory::index::collection_for;
use crate::memory::{COLLECTION_BRAIN, COLLECTION_MEMORY};
use std::path::Path;

/// Every brain file routes to the brain collection, wherever it sits on disk.
#[test]
fn brain_files_route_to_the_brain_collection() {
    for name in crate::memory::BRAIN_FILES {
        let path = Path::new("/home/someone/.opencrabs").join(name);
        assert_eq!(
            collection_for(&path),
            COLLECTION_BRAIN,
            "{name} must index into the collection search_brain reads"
        );
    }
}

/// Daily notes stay in the memory collection.
#[test]
fn daily_notes_route_to_the_memory_collection() {
    for name in ["2026-08-12.md", "2026-03-02.md"] {
        let path = Path::new("/home/someone/.opencrabs/memory").join(name);
        assert_eq!(collection_for(&path), COLLECTION_MEMORY, "{name}");
    }
}

/// Anything else is memory, which is the safe default: a stray file in the
/// brain collection would surface in `search_brain` as though it were a rule.
#[test]
fn unknown_files_default_to_the_memory_collection() {
    for name in ["scratch.md", "AGENTVERSE.md", "notes/idea.md"] {
        let path = Path::new("/home/someone/.opencrabs").join(name);
        assert_eq!(collection_for(&path), COLLECTION_MEMORY, "{name}");
    }
}

/// The routing is by file name, so the same brain file reached through a
/// different parent path still lands in the brain collection. Profiles give
/// each workspace its own home, so the parent differs per profile.
#[test]
fn routing_is_by_name_not_by_parent_directory() {
    let a = Path::new("/home/someone/.opencrabs/AGENTS.md");
    let b = Path::new("/home/someone/.opencrabs/profiles/ops/AGENTS.md");
    assert_eq!(collection_for(a), collection_for(b));
    assert_eq!(collection_for(a), COLLECTION_BRAIN);
}
