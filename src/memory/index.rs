//! Indexing — insert memory/brain files into the memory store and generate embeddings.

use super::db::Store;
use std::path::Path;
use std::sync::Mutex;

use super::embedding::{backfill_embeddings, embed_content, embed_content_api, embed_via_api};
use super::{COLLECTION_BRAIN, COLLECTION_MEMORY, embedding_api_config, embedding_api_configured};

/// Brain files loaded from the workspace root (`~/.opencrabs/`).
pub const BRAIN_FILES: &[&str] = &[
    "SOUL.md",
    "USER.md",
    "AGENTS.md",
    "TOOLS.md",
    "CODE.md",
    "SECURITY.md",
    "MEMORY.md",
    "BOOT.md",
    "HEARTBEAT.md",
];

/// Index a single `.md` file into the memory store under the right collection.
///
/// Skips re-indexing if the file's SHA-256 hash hasn't changed.
/// Generates an embedding when the engine is already initialized.
/// FTS-only index of one file: no embedding, no GPU.
///
/// The freshness check on the search path must not enter the embedding
/// backend. llama.cpp's Metal device asserts at process teardown when a
/// resource set is still outstanding, so every extra place that can be
/// embedding when the process exits is another way to abort on shutdown
/// (#1021). Embedding still happens where it always did — startup reindex and
/// the write path — which are bounded and not driven by user input.
///
/// FTS is what ranks a brain search, so refreshing it alone keeps the result
/// correct; a vector row that lags by one restart does not.
pub async fn index_file_fts_only(store: &'static Mutex<Store>, path: &Path) -> Result<(), String> {
    let body = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let s = store
            .lock()
            .map_err(|e| format!("Store lock poisoned: {e}"))?;
        index_file_sync(&s, collection_for(&path), &path, &body)?;
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| format!("spawn_blocking failed: {e}"))?
}

pub async fn index_file(store: &'static Mutex<Store>, path: &Path) -> Result<(), String> {
    let body = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;

    let path = path.to_path_buf();
    let body_clone = body.clone();

    // Phase 1: synchronous FTS indexing (blocking)
    let indexed = tokio::task::spawn_blocking(move || {
        let indexed = {
            let s = store
                .lock()
                .map_err(|e| format!("Store lock poisoned: {e}"))?;
            // Brain files live in their own collection, and `search_brain`
            // only looks there. Filing one under COLLECTION_MEMORY created a
            // second row that the next reindex deactivated, while the brain
            // row it should have updated went untouched — so an incremental
            // index of a brain file silently did nothing for brain search
            // (#1018 follow-up).
            index_file_sync(&s, collection_for(&path), &path, &body)?
        };
        Ok::<bool, String>(indexed)
    })
    .await
    .map_err(|e| format!("spawn_blocking failed: {e}"))??;

    // Phase 2: embedding (async for API, blocking for local)
    //
    // #1062: detached, never awaited by the caller. This used to be awaited
    // inline, and write_opencrabs_file awaits index_file, so a slow or
    // blackholed embedding path held the write tool hostage even though
    // Phase 1 FTS (the searchable index) was already complete. The API
    // branch now also has per-call timeouts (embed_via_api), so this task
    // always terminates; anything it misses is caught by the backfill on
    // next reindex. Sequential per-chunk embedding stays sequential within
    // the task.
    if indexed {
        let store_ref = store;
        tokio::spawn(async move {
            if embedding_api_configured() {
                if let Err(e) = embed_content_api(store_ref, &body_clone).await {
                    tracing::warn!("API embedding skipped during index: {e}");
                }
            } else {
                let body_for_embed = body_clone;
                if let Err(e) =
                    tokio::task::spawn_blocking(move || embed_content(store_ref, &body_for_embed))
                        .await
                        .map_err(|e| format!("spawn_blocking failed: {e}"))
                        .and_then(|r| r)
                {
                    tracing::warn!("Embedding skipped during index: {e}");
                }
            }
        });
    }

    Ok(())
}

/// Which collection a path belongs to.
///
/// `reindex` has always split these — daily notes into `COLLECTION_MEMORY`,
/// brain files into `COLLECTION_BRAIN` — but `index_file`, the incremental
/// path, hardcoded memory. Brain files therefore landed in the wrong
/// collection, and `search_brain` never saw them.
pub(crate) fn collection_for(path: &Path) -> &'static str {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if super::BRAIN_FILES.contains(&name) {
        COLLECTION_BRAIN
    } else {
        COLLECTION_MEMORY
    }
}

/// Synchronous inner implementation for indexing a single file into a given collection.
/// Returns `true` if new content was indexed, `false` if hash-skipped.
fn index_file_sync(
    store: &Store,
    collection: &str,
    path: &Path,
    body: &str,
) -> Result<bool, String> {
    let rel_path = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string());
    index_file_sync_keyed(store, collection, &rel_path, body)
}

/// Index one document under an explicit key.
///
/// Brain/memory use the basename (their corpora are single flat dirs); the
/// external collection uses absolute canonical paths so identically-named
/// files in different directories never collide (#1051).
///
/// Returns `true` if new content was indexed, `false` if hash-skipped.
pub(crate) fn index_file_sync_keyed(
    store: &Store,
    collection: &str,
    doc_key: &str,
    body: &str,
) -> Result<bool, String> {
    let hash = Store::hash_content(body);

    if let Ok(Some((_id, existing_hash, _title))) =
        store.find_active_document(collection, doc_key)
        && existing_hash == hash
    {
        return Ok(false);
    }

    let now = crate::utils::string::utc_timestamp();
    let title = Store::extract_title(body);

    // Pre-clear any existing FTS entry so the ON CONFLICT UPDATE branch in
    // insert_document fires a plain INSERT into documents_fts (not OR REPLACE,
    // which SQLite FTS5 rejects with "constraint failed").
    // Safe for new documents: deactivate_document matches 0 rows → no-op.
    let _ = store.deactivate_document(collection, doc_key);

    store
        .insert_content(&hash, body, &now)
        .map_err(|e| format!("Failed to insert content: {e}"))?;
    store
        .insert_document(collection, doc_key, &title, &hash, &now, &now)
        .map_err(|e| format!("Failed to insert document: {e}"))?;

    tracing::debug!("Indexed {collection} document: {doc_key}");
    Ok(true)
}

/// Walk `~/.opencrabs/memory/*.md` and `~/.opencrabs/*.md` brain files, indexing all.
///
/// Also deactivates entries for files that no longer exist on disk.
/// After indexing, backfills embeddings for any documents missing them.
/// Returns the number of files indexed.
pub async fn reindex(store: &'static Mutex<Store>) -> Result<usize, String> {
    let home = crate::config::opencrabs_home();
    let dir = home.join("memory");
    let mut indexed = 0usize;
    let mut memory_on_disk: Vec<String> = Vec::new();
    let mut brain_on_disk: Vec<String> = Vec::new();

    // --- Index daily memory logs ---
    if dir.exists() {
        let entries =
            std::fs::read_dir(&dir).map_err(|e| format!("Failed to read memory dir: {e}"))?;

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("md") {
                let rel = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                memory_on_disk.push(rel);

                if let Err(e) = index_file(store, &path).await {
                    tracing::warn!("Failed to index {}: {}", path.display(), e);
                } else {
                    indexed += 1;
                }
            }
        }
    }

    // --- Index brain workspace files ---
    for &name in BRAIN_FILES {
        let path = home.join(name);
        if path.exists() {
            let body = match tokio::fs::read_to_string(&path).await {
                Ok(b) if !b.trim().is_empty() => b,
                _ => continue,
            };
            brain_on_disk.push(name.to_string());

            let result: Result<bool, String> = tokio::task::spawn_blocking({
                let path = path.clone();
                move || {
                    let store = store
                        .lock()
                        .map_err(|e| format!("Store lock poisoned: {e}"))?;
                    index_file_sync(&store, COLLECTION_BRAIN, &path, &body)
                }
            })
            .await
            .map_err(|e| format!("spawn_blocking failed: {e}"))?;

            match result {
                Ok(_) => indexed += 1,
                Err(e) => tracing::warn!("Failed to index brain file {name}: {e}"),
            }
        }
    }

    // --- Prune deleted files from both collections ---
    let prune_result: Result<(), String> = tokio::task::spawn_blocking({
        move || {
            let store = store
                .lock()
                .map_err(|e| format!("Store lock poisoned: {e}"))?;

            if let Ok(db_paths) = store.get_active_document_paths(COLLECTION_MEMORY) {
                for db_path in &db_paths {
                    if !memory_on_disk.contains(db_path) {
                        let _ = store.deactivate_document(COLLECTION_MEMORY, db_path);
                        tracing::debug!("Pruned missing memory file: {}", db_path);
                    }
                }
            }

            if let Ok(db_paths) = store.get_active_document_paths(COLLECTION_BRAIN) {
                for db_path in &db_paths {
                    if !brain_on_disk.contains(db_path) {
                        let _ = store.deactivate_document(COLLECTION_BRAIN, db_path);
                        tracing::debug!("Pruned missing brain file: {}", db_path);
                    }
                }
            }

            Ok(())
        }
    })
    .await
    .map_err(|e| format!("spawn_blocking failed: {e}"))?;

    if let Err(e) = prune_result {
        tracing::warn!("Memory prune failed: {e}");
    }

    // --- Index external paths (#1051) ---
    // FTS-only here; embeddings ride the backfill below. The report carries
    // every problem (missing / unreadable / nested roots) so nothing is
    // silent. The periodic sweep reuses this same path, so extra_paths
    // config changes reconcile within one interval (Q15) and file deletion
    // is covered by the same prune.
    let external_report = match tokio::task::spawn_blocking({
        move || {
            let store = store
                .lock()
                .map_err(|e| format!("Store lock poisoned: {e}"))?;
            Ok::<_, String>(super::external::reindex_external(&store))
        }
    })
    .await
    {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => {
            tracing::warn!("memory: external reindex failed: {e}");
            super::external::ExternalReport::default()
        }
        Err(e) => {
            tracing::warn!("memory: external reindex join failed: {e}");
            super::external::ExternalReport::default()
        }
    };
    external_report.log();
    indexed += external_report.indexed;

    // --- Backfill embeddings for documents missing them ---
    if embedding_api_configured() {
        // API path: backfill one-by-one via HTTP (async)
        let store_ref = store;
        tokio::task::spawn_blocking(move || {
            let needing = match store_ref.lock() {
                Ok(s) => s.get_hashes_needing_embedding().unwrap_or_default(),
                Err(_) => return,
            };
            if needing.is_empty() {
                return;
            }
            tracing::info!("API backfill: {} documents need embeddings", needing.len());
            // Note: actual API calls happen in the next spawn_blocking cycle
            // to avoid blocking the tokio runtime. For now, just log.
            drop(needing);
        })
        .await
        .map_err(|e| format!("spawn_blocking failed: {e}"))?;

        // Async backfill: spawn as a background task
        let home = crate::config::opencrabs_home();
        tokio::spawn(async move {
            let store_ref = store;
            // Retire pre-chunking placeholders before listing what needs work
            // (#998). This was wired only into the local backfill at first,
            // which meant an install configured for an embedding API — where
            // THIS path runs and the local one never does — kept its
            // placeholders forever and never re-embedded a single document.
            match super::store::clear_skipped_placeholders() {
                Ok(0) => {}
                Ok(n) => tracing::info!("API backfill: reopened {n} previously skipped documents"),
                Err(e) => tracing::warn!("API backfill: placeholder sweep failed, continuing: {e}"),
            }

            // Retire pre-chunking placeholders before listing what needs work
            // (#1001). The sweep was wired only into the LOCAL backfill, which
            // meant an install configured for an embedding API — where THIS
            // path runs and the local one never does — kept its placeholders
            // forever and re-embedded nothing.
            match super::store::clear_skipped_placeholders() {
                Ok(0) => {}
                Ok(n) => tracing::info!("API backfill: reopened {n} previously skipped documents"),
                Err(e) => tracing::warn!("API backfill: placeholder sweep failed, continuing: {e}"),
            }

            // Get hashes needing embedding
            let needing = tokio::task::spawn_blocking(move || {
                store_ref
                    .lock()
                    .ok()
                    .and_then(|s| s.get_hashes_needing_embedding().ok())
                    .unwrap_or_default()
            })
            .await
            .unwrap_or_default();

            if needing.is_empty() {
                return;
            }

            let count = needing.len();
            tracing::info!("API backfill: embedding {count} documents");
            let mut stored = 0usize;

            for (hash, path, body) in &needing {
                // Chunked like every other embed path (#998). This one was
                // missed in the first pass: it is a second API backfill living
                // in the indexer rather than in `embedding.rs`, and it kept
                // writing `skipped-too-large` placeholders for anything past
                // 32 KB, which is exactly what made those documents
                // permanently unembeddable.
                let model_name = embedding_api_config()
                    .and_then(|c| c.model)
                    .unwrap_or_else(|| "api-embedding".to_string());
                let mut chunks_stored = 0usize;

                for (seq, chunk) in super::embedding::chunks_for(body).into_iter().enumerate() {
                    match embed_via_api(&chunk.text).await {
                        Ok(embedding) => {
                            let now = crate::utils::string::utc_timestamp();
                            if let Ok(s) = store_ref.lock()
                                && s.insert_embedding(
                                    hash,
                                    seq,
                                    chunk.pos,
                                    &embedding,
                                    &model_name,
                                    &now,
                                )
                                .is_ok()
                            {
                                chunks_stored += 1;
                            }
                        }
                        Err(e) => {
                            tracing::warn!("API embed failed for '{path}' chunk {seq}: {e}");
                        }
                    }
                }

                if chunks_stored > 0 {
                    stored += 1;
                }
            }
            tracing::info!("API backfilled {stored}/{count} embeddings");
            drop(home);
        });
    } else {
        // Local path: use GGUF engine (blocking)
        tokio::task::spawn_blocking(move || backfill_embeddings(store))
            .await
            .map_err(|e| format!("spawn_blocking failed: {e}"))?;
    }

    tracing::info!("Memory reindex complete: {} files", indexed);
    Ok(indexed)
}
