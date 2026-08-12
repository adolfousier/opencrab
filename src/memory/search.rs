//! Search — hybrid FTS5 + vector search via Reciprocal Rank Fusion.

use qmd::{SearchResult, Store, hybrid_search_rrf};
use std::path::Path;
use std::sync::Mutex;

use super::embedding::{embed_query_api, engine_if_ready};
use super::{COLLECTION_BRAIN, MemoryResult, embedding_api_configured};

/// Hybrid search across memory logs: FTS5 (BM25) + vector (cosine) via RRF.
///
/// Falls back to FTS-only when the embedding engine is unavailable.
/// Returns up to `n` results sorted by relevance.
pub async fn search(
    store: &'static Mutex<Store>,
    query: &str,
    n: usize,
) -> Result<Vec<MemoryResult>, String> {
    // Refresh brain files whose mtime moved since indexing (#1018). The index
    // was a boot-time snapshot, so a rule written mid-session was invisible
    // here until the next restart — precisely when a duplicate check needs it.
    // Stat-only for unchanged files, single-flight guarded, never fatal.
    super::freshness::refresh_stale_brain_files().await;

    let fts_query = sanitize_fts_query(query);
    if fts_query.is_empty() {
        return Ok(vec![]);
    }

    let query_owned = query.to_string();

    // API path: embed query via HTTP before entering spawn_blocking
    let api_embedding = if embedding_api_configured() {
        match embed_query_api(query).await {
            Ok(emb) => Some(emb),
            Err(e) => {
                tracing::warn!("API embedding failed for query, falling back to FTS-only: {e}");
                None
            }
        }
    } else {
        None
    };

    tokio::task::spawn_blocking(move || {
        // Local engine path: embed query via GGUF
        let query_embedding: Option<Vec<f32>> = if !embedding_api_configured() {
            engine_if_ready().and_then(|em| {
                em.lock()
                    .ok()
                    .and_then(|mut e| e.embed_query(&query_owned).ok().map(|r| r.embedding))
            })
        } else {
            api_embedding // Use the pre-fetched API embedding
        };

        // Store lock → search
        let store = store
            .lock()
            .map_err(|e| format!("Store lock poisoned: {e}"))?;
        let home = crate::config::opencrabs_home();

        let fts_results = store
            .search_fts(&fts_query, n, None)
            .map_err(|e| format!("FTS search failed: {e}"))?;

        // Hybrid path: combine FTS + vector results via Reciprocal Rank Fusion
        if let Some(ref query_emb) = query_embedding {
            // Chunk-aware (#998). qmd's own `search_vec` joins on `hash || '_0'`
            // and therefore only ever sees a document's first chunk, which would
            // make chunked embeddings write-only.
            let db_path = super::store::memory_dir().join("memory.db");
            let vec_hits = super::vector_search::search_chunks(&db_path, query_emb, n, None)
                .unwrap_or_default();

            if !vec_hits.is_empty() {
                let fts_tuples =
                    results_to_tuples_for(&store, &home, &fts_results, Some(&fts_query));
                let vec_tuples = chunk_hits_to_tuples(&store, &home, &vec_hits);
                let rrf = hybrid_search_rrf(fts_tuples, vec_tuples, 60);

                return Ok(rrf
                    .into_iter()
                    .take(n)
                    .map(|r| MemoryResult {
                        path: r.file,
                        snippet: extract_snippet(&r.body, &fts_query, 200),
                        rank: r.score,
                    })
                    .collect());
            }
        }

        // FTS-only fallback
        Ok(fts_results
            .iter()
            .map(|r| {
                let snippet = match store.get_document(&r.doc.collection_name, &r.doc.path) {
                    Ok(Some(doc)) => {
                        let body = doc.body.as_deref().unwrap_or("");
                        extract_snippet(body, &fts_query, 200)
                    }
                    _ => r.doc.title.clone(),
                };
                MemoryResult {
                    path: resolve_path(&home, &r.doc.collection_name, &r.doc.path),
                    snippet,
                    rank: r.score,
                }
            })
            .collect())
    })
    .await
    .map_err(|e| format!("spawn_blocking failed: {e}"))?
}

/// FTS-only search over the brain-file collection (no vector overhead).
///
/// Sub-millisecond BM25 over the indexed brain files (SOUL/USER/AGENTS/TOOLS/
/// CODE/SECURITY/MEMORY/BOOT/HEARTBEAT). Used by the harness brain-hints layer
/// (#767) to inject relevant guidance into tool errors and `tool_search`
/// results without paying the embedding round-trip the hybrid `search` does.
pub async fn search_brain(
    store: &'static Mutex<Store>,
    query: &str,
    n: usize,
) -> Result<Vec<MemoryResult>, String> {
    // Refresh brain files whose mtime moved since indexing (#1018). The index
    // was a boot-time snapshot, so a rule written mid-session was invisible
    // here until the next restart — precisely when a duplicate check needs it.
    // Stat-only for unchanged files, single-flight guarded, never fatal.
    super::freshness::refresh_stale_brain_files().await;

    let fts_query = sanitize_fts_query(query);
    if fts_query.is_empty() {
        return Ok(vec![]);
    }

    tokio::task::spawn_blocking(move || {
        let store = store
            .lock()
            .map_err(|e| format!("Store lock poisoned: {e}"))?;
        let home = crate::config::opencrabs_home();

        let fts_results = store
            .search_fts(&fts_query, n, Some(COLLECTION_BRAIN))
            .map_err(|e| format!("FTS search failed: {e}"))?;

        Ok(fts_results
            .iter()
            .map(|r| {
                let snippet = match store.get_document(&r.doc.collection_name, &r.doc.path) {
                    Ok(Some(doc)) => {
                        let body = doc.body.as_deref().unwrap_or("");
                        extract_snippet(body, &fts_query, 200)
                    }
                    _ => r.doc.title.clone(),
                };
                MemoryResult {
                    path: resolve_path(&home, &r.doc.collection_name, &r.doc.path),
                    snippet,
                    rank: r.score,
                }
            })
            .collect())
    })
    .await
    .map_err(|e| format!("spawn_blocking failed: {e}"))?
}

/// Convert chunk hits to RRF tuple format: (file_path, display_path, title, body).
///
/// The BODY is the whole document, not the matching chunk. The chunk decided
/// WHICH document is relevant; the caller still snippets the full text, and
/// handing back only the chunk would lose the surrounding context that makes a
/// snippet readable.
fn chunk_hits_to_tuples(
    store: &Store,
    home: &Path,
    hits: &[super::vector_search::ChunkHit],
) -> Vec<(String, String, String, String)> {
    hits.iter()
        .map(|h| {
            let file_path = resolve_path(home, &h.collection, &h.path);
            let body = store
                .get_document(&h.collection, &h.path)
                .ok()
                .flatten()
                .and_then(|d| d.body)
                .unwrap_or_default();
            (file_path.clone(), file_path, h.title.clone(), body)
        })
        .collect()
}

/// Convert SearchResults to RRF tuple format: (file_path, display_path, title,
/// body), narrowing each hit to its best matching chunk when a query is given
/// (#1000).
///
/// `search_fts` matches whole documents, so without this the lexical half of
/// hybrid search ranks files while the vector half ranks chunks, and RRF fuses
/// two lists describing different units. Passing the query narrows the body to
/// the passage that earned the hit, which is also what the snippet should be
/// cut from.
fn results_to_tuples_for(
    store: &Store,
    home: &Path,
    results: &[SearchResult],
    query: Option<&str>,
) -> Vec<(String, String, String, String)> {
    results
        .iter()
        .map(|r| {
            let file_path = resolve_path(home, &r.doc.collection_name, &r.doc.path);
            let full = store
                .get_document(&r.doc.collection_name, &r.doc.path)
                .ok()
                .flatten()
                .and_then(|d| d.body)
                .unwrap_or_default();
            let body = match query.and_then(|q| super::chunk_fts::best_chunk(&full, q)) {
                Some((_, chunk)) => chunk,
                None => full,
            };
            (
                file_path,
                r.doc.display_path.clone(),
                r.doc.title.clone(),
                body,
            )
        })
        .collect()
}

/// Resolve filesystem path for a search result based on its collection.
fn resolve_path(home: &Path, collection: &str, doc_path: &str) -> String {
    let p = if collection == COLLECTION_BRAIN {
        home.join(doc_path)
    } else {
        home.join("memory").join(doc_path)
    };
    p.to_string_lossy().to_string()
}

/// Sanitize a search query for FTS5: wrap each word in double quotes
/// to avoid syntax errors from special characters, then join with spaces (implicit AND).
pub(crate) fn sanitize_fts_query(query: &str) -> String {
    query
        .split_whitespace()
        .map(|w| {
            let clean: String = w.chars().filter(|c| *c != '"').collect();
            format!("\"{clean}\"")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Extract a snippet from body text around the first query term match.
pub(crate) fn extract_snippet(body: &str, query: &str, max_len: usize) -> String {
    let query_lower = query.to_lowercase();
    let body_lower = body.to_lowercase();

    let mut best_pos = 0;
    for word in query_lower.split_whitespace() {
        let clean: String = word.chars().filter(|c| *c != '"').collect();
        if !clean.is_empty()
            && let Some(pos) = body_lower.find(&clean)
        {
            best_pos = pos;
            break;
        }
    }

    let start = best_pos.saturating_sub(50);
    let end = (start + max_len).min(body.len());

    let start = body.floor_char_boundary(start);
    let end = body.ceil_char_boundary(end);

    let mut snippet = String::new();
    if start > 0 {
        snippet.push_str("...");
    }
    snippet.push_str(body[start..end].trim());
    if end < body.len() {
        snippet.push_str("...");
    }

    snippet
}
