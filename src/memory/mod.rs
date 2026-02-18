//! Memory Module
//!
//! Provides long-term memory search via the `qmd` crate's built-in FTS5 engine.
//! Memory logs (`~/.opencrabs/memory/YYYY-MM-DD.md`) are indexed into a qmd Store
//! for fast BM25-ranked retrieval.

use once_cell::sync::OnceCell;
use qmd::Store;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// A single search result from the memory index.
#[derive(Debug, Clone)]
pub struct MemoryResult {
    pub path: String,
    pub snippet: String,
    pub rank: f64,
}

/// Collection name for daily compaction logs.
const COLLECTION_MEMORY: &str = "memory";
/// Collection name for workspace brain files (SOUL.md, MEMORY.md, etc.).
const COLLECTION_BRAIN: &str = "brain";

/// Lazy-initialized singleton qmd Store for the memory database.
static STORE: OnceCell<Mutex<Store>> = OnceCell::new();

/// Get (or create) the shared memory qmd Store.
///
/// The database lives at `~/.opencrabs/memory/memory.db`.
/// First call initializes the schema via `Store::open`.
pub fn get_store() -> Result<&'static Mutex<Store>, String> {
    STORE.get_or_try_init(|| {
        let db_path = memory_dir().join("memory.db");

        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create memory dir: {e}"))?;
        }

        let store = Store::open(&db_path)
            .map_err(|e| format!("Failed to open memory store: {e}"))?;

        tracing::info!("Memory qmd store ready at {}", db_path.display());
        Ok(Mutex::new(store))
    })
}

/// Full-text search across memory logs using qmd FTS5 BM25 ranking.
///
/// Returns up to `n` results sorted by relevance.
pub async fn search(
    store: &'static Mutex<Store>,
    query: &str,
    n: usize,
) -> Result<Vec<MemoryResult>, String> {
    let fts_query = sanitize_fts_query(query);
    if fts_query.is_empty() {
        return Ok(vec![]);
    }

    tokio::task::spawn_blocking(move || {
        let store = store.lock().map_err(|e| format!("Store lock poisoned: {e}"))?;

        // Search across all collections (memory logs + brain files)
        let results = store
            .search_fts(&fts_query, n, None)
            .map_err(|e| format!("FTS search failed: {e}"))?;

        let home = crate::config::opencrabs_home();
        let mut memory_results = Vec::new();
        for r in results {
            // Fetch document body for snippet extraction
            let snippet =
                match store.get_document(&r.doc.collection_name, &r.doc.path) {
                    Ok(Some(doc)) => {
                        let body = doc.body.as_deref().unwrap_or("");
                        extract_snippet(body, &fts_query, 200)
                    }
                    _ => r.doc.title.clone(),
                };

            // Resolve filesystem path based on collection
            let file_path = if r.doc.collection_name == COLLECTION_BRAIN {
                home.join(&r.doc.path)
            } else {
                home.join("memory").join(&r.doc.path)
            };
            memory_results.push(MemoryResult {
                path: file_path.to_string_lossy().to_string(),
                snippet,
                rank: r.score,
            });
        }

        Ok(memory_results)
    })
    .await
    .map_err(|e| format!("spawn_blocking failed: {e}"))?
}

/// Index a single `.md` file into the qmd store under the `"memory"` collection.
///
/// Skips re-indexing if the file's SHA-256 hash hasn't changed.
pub async fn index_file(store: &'static Mutex<Store>, path: &Path) -> Result<(), String> {
    let body = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;

    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let store = store.lock().map_err(|e| format!("Store lock poisoned: {e}"))?;
        index_file_sync(&store, COLLECTION_MEMORY, &path, &body)
    })
    .await
    .map_err(|e| format!("spawn_blocking failed: {e}"))?
}

/// Synchronous inner implementation for indexing a single file into a given collection.
fn index_file_sync(
    store: &Store,
    collection: &str,
    path: &Path,
    body: &str,
) -> Result<(), String> {
    let hash = Store::hash_content(body);
    let rel_path = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string());

    // Check if already indexed with same hash
    if let Ok(Some((_id, existing_hash, _title))) =
        store.find_active_document(collection, &rel_path)
        && existing_hash == hash
    {
        return Ok(()); // unchanged
    }

    let now = chrono::Local::now()
        .format("%Y-%m-%dT%H:%M:%S")
        .to_string();
    let title = Store::extract_title(body);

    store
        .insert_content(&hash, body, &now)
        .map_err(|e| format!("Failed to insert content: {e}"))?;
    store
        .insert_document(collection, &rel_path, &title, &hash, &now, &now)
        .map_err(|e| format!("Failed to insert document: {e}"))?;

    tracing::debug!("Indexed {collection} file: {}", path.display());
    Ok(())
}

/// Brain files loaded from the workspace root (`~/.opencrabs/`).
const BRAIN_FILES: &[&str] = &[
    "SOUL.md",
    "IDENTITY.md",
    "USER.md",
    "AGENTS.md",
    "TOOLS.md",
    "SECURITY.md",
    "MEMORY.md",
    "BOOT.md",
    "BOOTSTRAP.md",
    "HEARTBEAT.md",
];

/// Walk `~/.opencrabs/memory/*.md` and `~/.opencrabs/*.md` brain files, indexing all.
///
/// Also deactivates entries for files that no longer exist on disk.
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

            let result: Result<(), String> = tokio::task::spawn_blocking({
                let path = path.clone();
                move || {
                    let store =
                        store.lock().map_err(|e| format!("Store lock poisoned: {e}"))?;
                    index_file_sync(&store, COLLECTION_BRAIN, &path, &body)
                }
            })
            .await
            .map_err(|e| format!("spawn_blocking failed: {e}"))?;

            match result {
                Ok(()) => indexed += 1,
                Err(e) => tracing::warn!("Failed to index brain file {name}: {e}"),
            }
        }
    }

    // --- Prune deleted files from both collections ---
    let prune_result: Result<(), String> = tokio::task::spawn_blocking({
        move || {
            let store = store.lock().map_err(|e| format!("Store lock poisoned: {e}"))?;

            // Prune memory collection
            if let Ok(db_paths) = store.get_active_document_paths(COLLECTION_MEMORY) {
                for db_path in &db_paths {
                    if !memory_on_disk.contains(db_path) {
                        let _ = store.deactivate_document(COLLECTION_MEMORY, db_path);
                        tracing::debug!("Pruned missing memory file: {}", db_path);
                    }
                }
            }

            // Prune brain collection
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

    tracing::info!("Memory reindex complete: {} files", indexed);
    Ok(indexed)
}

/// Sanitize a search query for FTS5: wrap each word in double quotes
/// to avoid syntax errors from special characters, then join with spaces (implicit AND).
fn sanitize_fts_query(query: &str) -> String {
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
fn extract_snippet(body: &str, query: &str, max_len: usize) -> String {
    let query_lower = query.to_lowercase();
    let body_lower = body.to_lowercase();

    // Find first occurrence of any query word
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

    // Snap to char boundaries
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

/// Path to the memory directory: `~/.opencrabs/memory/`
fn memory_dir() -> PathBuf {
    crate::config::opencrabs_home().join("memory")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_dir() {
        let dir = memory_dir();
        assert!(dir.to_string_lossy().contains("memory"));
    }

    #[test]
    fn test_sanitize_fts_query() {
        assert_eq!(sanitize_fts_query("hello world"), "\"hello\" \"world\"");
        assert_eq!(sanitize_fts_query(""), "");
        assert_eq!(sanitize_fts_query("auth\"bug"), "\"authbug\"");
    }

    #[test]
    fn test_extract_snippet() {
        let body =
            "# Today\nFixed the authentication bug in login flow. Also refactored database.";
        let snippet = extract_snippet(body, "\"authentication\"", 60);
        assert!(snippet.contains("authentication"));
    }

    #[test]
    fn test_extract_snippet_no_match() {
        let body = "Some content without the search term";
        let snippet = extract_snippet(body, "\"nonexistent\"", 60);
        // Should return from start of body
        assert!(snippet.contains("Some content"));
    }

    #[test]
    fn test_index_and_search_integration() {
        // Test that Store::open works with a temp directory
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let store = Store::open(&db_path).unwrap();

        // Index a document
        let body = "# Session\nFixed the authentication bug in login flow";
        let hash = Store::hash_content(body);
        let now = "2024-01-01T00:00:00";
        let title = Store::extract_title(body);

        store.insert_content(&hash, body, now).unwrap();
        store
            .insert_document("test", "2024-01-01.md", &title, &hash, now, now)
            .unwrap();

        // Search should find it
        let results = store.search_fts("\"authentication\"", 5, Some("test")).unwrap();
        assert!(!results.is_empty());

        // Hash-based skip: find_active_document returns same hash
        let found = store
            .find_active_document("test", "2024-01-01.md")
            .unwrap();
        assert!(found.is_some());
        let (_id, found_hash, _title) = found.unwrap();
        assert_eq!(found_hash, hash);
    }
}
