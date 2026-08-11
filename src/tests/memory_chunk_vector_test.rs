//! Chunked embeddings are stored AND reachable (#998).
//!
//! Two halves, and either one alone is useless:
//!
//! 1. Content must be split before embedding. It was not, so a document became
//!    one averaged vector, and anything past 32 KB got a `skipped-too-large`
//!    placeholder and no vector at all. On a real workspace that was 25% of all
//!    vector rows, including the long-term memory file.
//! 2. Chunks past the first must be searchable. `qmd::Store::search_vec` joins
//!    on `hash || '_0'`, so writing chunk 1..N through qmd's own search would
//!    store data nothing ever queries.
//!
//! These tests use the store schema directly rather than the embedding engine,
//! which needs a ~300 MB model download and AVX. What is under test is the
//! chunk plumbing, not llama.cpp.

use rusqlite::Connection;
use tempfile::TempDir;

/// Build a store with the tables the vector search reads, and one document
/// carrying `n_chunks` embeddings.
///
/// `distinct_at` is the chunk given a vector that matches the probe query, so a
/// test can place the answer at any chunk index and check it is found.
fn store_with_chunks(dir: &TempDir, n_chunks: usize, distinct_at: usize) -> std::path::PathBuf {
    let db = dir.path().join("memory.db");
    let conn = Connection::open(&db).expect("open");
    conn.execute_batch(
        "
        CREATE TABLE documents (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            collection TEXT NOT NULL, path TEXT NOT NULL, title TEXT NOT NULL,
            hash TEXT NOT NULL, created_at TEXT NOT NULL, modified_at TEXT NOT NULL,
            active INTEGER NOT NULL DEFAULT 1, UNIQUE(collection, path));
        CREATE TABLE content_vectors (
            hash TEXT NOT NULL, seq INTEGER NOT NULL DEFAULT 0, pos INTEGER NOT NULL DEFAULT 0,
            model TEXT NOT NULL, embedded_at TEXT NOT NULL, PRIMARY KEY (hash, seq));
        CREATE TABLE vectors_vec (hash_seq TEXT PRIMARY KEY, embedding BLOB NOT NULL);
        INSERT INTO documents (collection, path, title, hash, created_at, modified_at, active)
        VALUES ('memory', 'doc.md', 'Doc', 'h1', 'now', 'now', 1);
        ",
    )
    .expect("schema");

    for seq in 0..n_chunks {
        // The matching chunk points along the query axis; the others point away.
        let vec: Vec<f32> = if seq == distinct_at {
            vec![1.0, 0.0, 0.0]
        } else {
            vec![0.0, 1.0, 0.0]
        };
        let blob: Vec<u8> = vec.iter().flat_map(|f| f.to_le_bytes()).collect();
        conn.execute(
            "INSERT INTO content_vectors (hash, seq, pos, model, embedded_at)
             VALUES ('h1', ?1, ?2, 'test-model', 'now')",
            rusqlite::params![seq as i64, (seq * 100) as i64],
        )
        .expect("insert cv");
        conn.execute(
            "INSERT INTO vectors_vec (hash_seq, embedding) VALUES (?1, ?2)",
            rusqlite::params![format!("h1_{seq}"), blob],
        )
        .expect("insert vec");
    }
    db
}

/// The regression: a match in a chunk other than the first must be found.
///
/// This is the whole reason the search was reimplemented. Under qmd's
/// `search_vec` the join is `hash || '_0'`, so this document would score on
/// chunk 0 (which points away from the query) and the real answer at chunk 4
/// would be invisible.
#[test]
fn a_match_in_a_later_chunk_is_found() {
    use crate::memory::vector_search::search_chunks;
    let dir = TempDir::new().unwrap();
    let db = store_with_chunks(&dir, 6, 4);

    let hits = search_chunks(&db, &[1.0, 0.0, 0.0], 10, None).expect("search");
    assert_eq!(hits.len(), 1, "one document, so one hit: {hits:?}");
    assert_eq!(
        hits[0].seq, 4,
        "the matching chunk must be the one returned"
    );
    assert_eq!(hits[0].pos, 400, "chunk offset must survive the round trip");
    assert!(hits[0].score > 0.9, "score was {}", hits[0].score);
}

/// One verbose document must not fill every slot with its own chunks.
#[test]
fn a_document_yields_at_most_one_hit() {
    use crate::memory::vector_search::search_chunks;
    let dir = TempDir::new().unwrap();
    // Every chunk matches the query this time.
    let db = store_with_chunks(&dir, 8, usize::MAX);

    let hits = search_chunks(&db, &[0.0, 1.0, 0.0], 10, None).expect("search");
    assert_eq!(
        hits.len(),
        1,
        "8 matching chunks of one document must collapse to one result: {hits:?}"
    );
}

/// Placeholder rows hold an empty vector and must never be scored.
#[test]
fn skipped_too_large_placeholders_are_never_returned() {
    use crate::memory::vector_search::search_chunks;
    let dir = TempDir::new().unwrap();
    let db = store_with_chunks(&dir, 1, 0);
    let conn = Connection::open(&db).unwrap();
    conn.execute(
        "UPDATE content_vectors SET model = 'skipped-too-large' WHERE hash = 'h1'",
        [],
    )
    .unwrap();

    let hits = search_chunks(&db, &[1.0, 0.0, 0.0], 10, None).expect("search");
    assert!(
        hits.is_empty(),
        "a zero-vector placeholder must not occupy a result slot: {hits:?}"
    );
}

/// A vector from a different embedding model has different dimensions and
/// cannot be compared. Skipping beats scoring it as 0.0 and taking a slot.
#[test]
fn vectors_of_a_different_dimension_are_skipped() {
    use crate::memory::vector_search::search_chunks;
    let dir = TempDir::new().unwrap();
    let db = store_with_chunks(&dir, 1, 0);

    let hits = search_chunks(&db, &[1.0, 0.0, 0.0, 0.0], 10, None).expect("search");
    assert!(
        hits.is_empty(),
        "a 3-dim stored vector must not be scored against a 4-dim query: {hits:?}"
    );
}

/// Collection scoping is what keeps separate bodies of content apart.
#[test]
fn collection_scoping_excludes_other_collections() {
    use crate::memory::vector_search::search_chunks;
    let dir = TempDir::new().unwrap();
    let db = store_with_chunks(&dir, 1, 0);

    let inside = search_chunks(&db, &[1.0, 0.0, 0.0], 10, Some("memory")).expect("search");
    assert_eq!(inside.len(), 1, "the document's own collection must match");

    let outside = search_chunks(&db, &[1.0, 0.0, 0.0], 10, Some("brain")).expect("search");
    assert!(
        outside.is_empty(),
        "a different collection must not return it: {outside:?}"
    );
}

// --- chunking itself --------------------------------------------------------

/// A document larger than one chunk is actually split.
///
/// The bug was not that chunking was broken, it was that it was never called:
/// all three embed sites passed `seq = 0, pos = 0`, so `max(seq)` across a real
/// 227 MB store was 0.
#[test]
fn long_content_is_split_into_overlapping_chunks() {
    use crate::memory::embedding::chunks_for;

    let body = "Paragraph about retrieval. ".repeat(1000); // ~27 KB
    let chunks = chunks_for(&body);

    assert!(
        chunks.len() > 1,
        "long content must produce several chunks, got {}",
        chunks.len()
    );
    // Positions advance, and by less than the chunk size, which is what
    // overlap means: a passage on a boundary survives whole in one chunk.
    for pair in chunks.windows(2) {
        assert!(
            pair[1].pos > pair[0].pos,
            "chunk offsets must advance: {} then {}",
            pair[0].pos,
            pair[1].pos
        );
    }
    let stride = chunks[1].pos - chunks[0].pos;
    assert!(
        stride < chunks[0].text.len(),
        "consecutive chunks must overlap: stride {stride} vs chunk len {}",
        chunks[0].text.len()
    );
}

/// Short content stays a single chunk, so ordinary notes are unaffected.
#[test]
fn short_content_stays_one_chunk() {
    use crate::memory::embedding::chunks_for;
    let chunks = chunks_for("A short memory note.");
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].pos, 0);
}

/// Every chunk fits under the llama.cpp size guard.
///
/// This is what makes `skipped-too-large` stop happening for ordinary content,
/// rather than needing the cap raised.
#[test]
fn no_chunk_approaches_the_embed_size_guard() {
    use crate::memory::embedding::chunks_for;
    let body = "x".repeat(500_000);
    for (i, chunk) in chunks_for(&body).into_iter().enumerate() {
        assert!(
            chunk.text.len() < 32_000,
            "chunk {i} is {} bytes, at or over the guard",
            chunk.text.len()
        );
    }
}

// --- lexical chunk refinement (#1000) ---------------------------------------

/// A document hit narrows to the passage that matched, not the whole file.
///
/// `search_fts` matches whole documents, so before this the lexical half told
/// you a 99 KB file was relevant and left the caller to snippet around the
/// first occurrence of a query term, which need not be the passage that
/// answers the question.
#[test]
fn a_document_narrows_to_its_best_chunk() {
    use crate::memory::chunk_fts::best_chunk;

    // The answer sits deep in a long document, well past the first chunk.
    let filler = "Unrelated background prose about scheduling and formatting. ".repeat(120);
    let body =
        format!("{filler}\n\nThe quota breaker trips after five consecutive refusals.\n\n{filler}");

    let (pos, chunk) = best_chunk(&body, "quota breaker consecutive refusals").expect("a chunk");
    assert!(
        chunk.contains("quota breaker trips"),
        "the returned chunk must contain the answer, got: {}",
        &chunk[..chunk.len().min(120)]
    );
    assert!(
        chunk.len() < body.len() / 2,
        "narrowing must actually narrow: chunk {} vs body {}",
        chunk.len(),
        body.len()
    );
    assert!(pos > 0, "the answer was past the first chunk, so pos > 0");
}

/// A short document has nothing to narrow, and says so.
///
/// `None` means "use the whole body", not "irrelevant". FTS already judged
/// relevance; this only decides where.
#[test]
fn a_single_chunk_document_needs_no_narrowing() {
    use crate::memory::chunk_fts::best_chunk;
    assert!(best_chunk("A short note about quotas.", "quotas").is_none());
}

/// Empty content cannot be narrowed.
#[test]
fn empty_content_yields_no_chunk() {
    use crate::memory::chunk_fts::best_chunk;
    assert!(best_chunk("", "anything").is_none());
    assert!(best_chunk("   \n\n  ", "anything").is_none());
}

/// Chunk-level lexical matching inherits diacritic folding.
///
/// This is why refinement reuses `section_rank` instead of growing its own
/// scorer: an unaccented query has to reach accented text here exactly as it
/// does in brain-file recall.
#[test]
fn chunk_refinement_folds_accents_like_the_rest_of_retrieval() {
    use crate::memory::chunk_fts::best_chunk;

    let filler = "Texto de relleno sobre otros temas del sistema. ".repeat(120);
    let body =
        format!("{filler}\n\nLa configuración del entorno se revisa cada semana.\n\n{filler}");

    let (_, chunk) = best_chunk(&body, "configuracion del entorno").expect("a chunk");
    assert!(
        chunk.contains("configuración del entorno"),
        "an unaccented query must reach the accented passage"
    );
}
