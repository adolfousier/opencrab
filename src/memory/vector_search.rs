//! Chunk-aware vector search over the memory store (#998).
//!
//! `qmd::Store::search_vec` joins `vectors_vec` on `d.hash || '_0'`, so it can
//! only ever see a document's FIRST chunk. Once documents are chunked, every
//! chunk past the first would be embedded, stored, and then never queried,
//! which is the worst of both worlds: the cost of chunking with none of the
//! benefit.
//!
//! qmd is a third-party crate and 0.3.2 is its latest release, so this reads
//! the same tables directly rather than waiting on an upstream change. It is
//! deliberately a read-only, additive shim:
//!
//! - Its own SQLite connection, opened read-only. The database runs in WAL
//!   mode, so a reader never blocks qmd's writer and vice versa.
//! - No schema of its own. It queries what qmd already writes, so nothing here
//!   has to be migrated if qmd's search gains chunk support later and this is
//!   deleted.
//!
//! Scoring matches qmd's: cosine similarity computed in Rust over f32 vectors
//! stored as little-endian bytes. `vectors_vec` is a plain table, not a
//! vector-index extension, so this is the same linear scan qmd performs.

use std::collections::HashMap;
use std::path::Path;

use rusqlite::{Connection, OpenFlags};

/// One matching chunk, identified by the document it belongs to.
#[derive(Debug, Clone)]
pub struct ChunkHit {
    pub collection: String,
    pub path: String,
    pub title: String,
    pub hash: String,
    /// Chunk index within the document.
    pub seq: usize,
    /// Character offset of the chunk in the document.
    pub pos: usize,
    /// Cosine similarity against the query.
    pub score: f32,
}

/// Placeholder model name written for content that could not be embedded.
///
/// Rows carrying it hold an empty vector, so they must never be scored: a
/// zero-length embedding would otherwise cosine to 0.0 and quietly occupy a
/// result slot.
const SKIPPED_MODEL: &str = "skipped-too-large";

/// Decode a little-endian f32 blob, the encoding qmd writes.
fn decode_embedding(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// Best-scoring chunk per document, ranked, for `query_embedding`.
///
/// Returns at most `limit` documents. Deduplicating to the best chunk is
/// deliberate: a long document produces many chunks, and without this a single
/// verbose file would fill every result slot with its own near-duplicates and
/// crowd out other documents entirely.
///
/// `collection` scopes the search when set, which is what keeps separate bodies
/// of indexed content from bleeding into each other's results.
pub fn search_chunks(
    db_path: &Path,
    query_embedding: &[f32],
    limit: usize,
    collection: Option<&str>,
) -> Result<Vec<ChunkHit>, String> {
    if query_embedding.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }

    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("Failed to open memory store for vector search: {e}"))?;

    // Bodies are deliberately not selected here. The scan touches every
    // embedded chunk, and joining document text onto it would pull the whole
    // corpus into memory to rank it.
    let sql = "
        SELECT d.collection, d.path, d.title, d.hash, cv.seq, cv.pos, v.embedding
        FROM documents d
        JOIN content_vectors cv ON cv.hash = d.hash
        JOIN vectors_vec v ON v.hash_seq = d.hash || '_' || cv.seq
        WHERE d.active = 1 AND cv.model <> ?1
          AND (?2 IS NULL OR d.collection = ?2)
    ";

    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| format!("Failed to prepare vector search: {e}"))?;

    let rows = stmt
        .query_map(rusqlite::params![SKIPPED_MODEL, collection], |row| {
            let embedding: Vec<u8> = row.get(6)?;
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)? as usize,
                row.get::<_, i64>(5)? as usize,
                embedding,
            ))
        })
        .map_err(|e| format!("Vector search query failed: {e}"))?;

    // Keep only the strongest chunk per document as we go, so peak memory is
    // bounded by the document count rather than the chunk count.
    let mut best: HashMap<(String, String), ChunkHit> = HashMap::new();
    for row in rows {
        let (collection, path, title, hash, seq, pos, blob) =
            row.map_err(|e| format!("Vector row decode failed: {e}"))?;

        let embedding = decode_embedding(&blob);
        if embedding.len() != query_embedding.len() {
            // A stored vector from a different embedding model. Comparing
            // across dimensions is meaningless, so skip rather than score it.
            continue;
        }

        let score = qmd::cosine_similarity(query_embedding, &embedding);
        let key = (collection.clone(), path.clone());
        let better = best.get(&key).is_none_or(|prev| score > prev.score);
        if better {
            best.insert(
                key,
                ChunkHit {
                    collection,
                    path,
                    title,
                    hash,
                    seq,
                    pos,
                    score,
                },
            );
        }
    }

    let mut hits: Vec<ChunkHit> = best.into_values().collect();
    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            // Stable tie-break so repeated identical queries agree.
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.seq.cmp(&b.seq))
    });
    hits.truncate(limit);
    Ok(hits)
}
