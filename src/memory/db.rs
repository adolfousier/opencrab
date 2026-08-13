//! Store — the memory database, owned by OpenCrabs.
//!
//! This replaced the `qmd` crate (qntx/qmd 0.3.2), which had not published a
//! release since Feb 2026 while every defect it ever shipped was found and
//! fixed on our side: byte-index chunking that panicked on multi-byte chars
//! (#1002), vector search that only ever saw chunk 0 (#998), doc-level-only
//! FTS (#1000). The audit (2026-08-12) mapped the entire surface we used —
//! 13 Store methods — and this file is that surface, nothing more: no rerank,
//! no generation, no collections admin, no llm_cache.
//!
//! The schema is qmd's verbatim on purpose. Every existing `memory.db` in the
//! wild was created by qmd, and `vector_search.rs` reads these tables
//! directly, so the DDL below IS the migration story: there isn't one.

use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// One indexed document row plus its body when loaded.
#[derive(Debug, Clone)]
pub struct DocumentResult {
    pub collection_name: String,
    pub path: String,
    pub display_path: String,
    pub title: String,
    pub hash: String,
    pub modified_at: String,
    pub body: Option<String>,
}

/// A scored search hit.
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub doc: DocumentResult,
    pub score: f64,
}

/// The database store: one connection per database file, owned by the caller
/// behind a Mutex (see `super::store`).
#[derive(Debug)]
pub struct Store {
    conn: Connection,
    db_path: PathBuf,
}

impl Store {
    /// Open (or create) the store at `db_path` and initialize the schema.
    pub fn open(db_path: &Path) -> Result<Self, String> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create store dir: {e}"))?;
        }

        let conn = Connection::open(db_path)
            .map_err(|e| format!("Failed to open {}: {e}", db_path.display()))?;

        // WAL lets `vector_search` read on its own read-only connection
        // while this one writes. The busy timeout covers the rare writer/
        // snapshot overlap without changing any observable behavior.
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| format!("Failed to set busy timeout: {e}"))?;

        let mut store = Self {
            conn,
            db_path: db_path.to_path_buf(),
        };
        store.initialize()?;
        Ok(store)
    }

    /// Database file path.
    #[must_use]
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    /// Schema DDL, byte-for-byte the one qmd 0.3.2 created. Do not "improve"
    /// it casually: existing memory.db files and vector_search.rs depend on
    /// exactly these tables, columns and triggers.
    fn initialize(&mut self) -> Result<(), String> {
        self.conn
            .execute_batch(
                r"
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;

            -- Content-addressable storage
            CREATE TABLE IF NOT EXISTS content (
                hash TEXT PRIMARY KEY,
                doc TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            -- Documents table
            CREATE TABLE IF NOT EXISTS documents (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                collection TEXT NOT NULL,
                path TEXT NOT NULL,
                title TEXT NOT NULL,
                hash TEXT NOT NULL,
                created_at TEXT NOT NULL,
                modified_at TEXT NOT NULL,
                active INTEGER NOT NULL DEFAULT 1,
                FOREIGN KEY (hash) REFERENCES content(hash) ON DELETE CASCADE,
                UNIQUE(collection, path)
            );

            CREATE INDEX IF NOT EXISTS idx_documents_collection ON documents(collection, active);
            CREATE INDEX IF NOT EXISTS idx_documents_hash ON documents(hash);
            CREATE INDEX IF NOT EXISTS idx_documents_path ON documents(path, active);

            -- FTS index
            CREATE VIRTUAL TABLE IF NOT EXISTS documents_fts USING fts5(
                filepath, title, body,
                tokenize='porter unicode61'
            );

            -- Content vectors metadata
            CREATE TABLE IF NOT EXISTS content_vectors (
                hash TEXT NOT NULL,
                seq INTEGER NOT NULL DEFAULT 0,
                pos INTEGER NOT NULL DEFAULT 0,
                model TEXT NOT NULL,
                embedded_at TEXT NOT NULL,
                PRIMARY KEY (hash, seq)
            );
            ",
            )
            .map_err(|e| format!("Failed to initialize schema: {e}"))?;

        self.create_fts_triggers()
    }

    /// FTS5 external-content sync triggers. Copied verbatim from qmd: the
    /// FTS index is keyed by documents.id (rowid) and mirrors active rows.
    fn create_fts_triggers(&self) -> Result<(), String> {
        let trigger_exists: bool = self
            .conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='trigger' AND name='documents_ai'",
                [],
                |_| Ok(true),
            )
            .unwrap_or(false);

        if !trigger_exists {
            self.conn
                .execute_batch(
                    r"
                CREATE TRIGGER IF NOT EXISTS documents_ai AFTER INSERT ON documents
                WHEN new.active = 1
                BEGIN
                    INSERT INTO documents_fts(rowid, filepath, title, body)
                    SELECT
                        new.id,
                        new.collection || '/' || new.path,
                        new.title,
                        (SELECT doc FROM content WHERE hash = new.hash)
                    WHERE new.active = 1;
                END;

                CREATE TRIGGER IF NOT EXISTS documents_ad AFTER DELETE ON documents BEGIN
                    DELETE FROM documents_fts WHERE rowid = old.id;
                END;

                CREATE TRIGGER IF NOT EXISTS documents_au AFTER UPDATE ON documents
                BEGIN
                    DELETE FROM documents_fts WHERE rowid = old.id AND new.active = 0;
                    INSERT OR REPLACE INTO documents_fts(rowid, filepath, title, body)
                    SELECT
                        new.id,
                        new.collection || '/' || new.path,
                        new.title,
                        (SELECT doc FROM content WHERE hash = new.hash)
                    WHERE new.active = 1;
                END;
                ",
                )
                .map_err(|e| format!("Failed to create FTS triggers: {e}"))?;
        }

        Ok(())
    }

    /// Hash content using SHA256. Must stay byte-compatible with hashes
    /// already stored (they are the foreign keys into `content`).
    #[must_use]
    pub fn hash_content(content: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Extract title from markdown content (first `#` or `##` heading).
    #[must_use]
    pub fn extract_title(content: &str) -> String {
        for line in content.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("# ") {
                return rest.trim().to_string();
            }
            if let Some(rest) = trimmed.strip_prefix("## ") {
                return rest.trim().to_string();
            }
        }
        String::new()
    }

    /// Insert content into content-addressable storage.
    pub fn insert_content(&self, hash: &str, content: &str, created_at: &str) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO content (hash, doc, created_at) VALUES (?1, ?2, ?3)",
                params![hash, content, created_at],
            )
            .map_err(|e| format!("insert_content: {e}"))?;
        Ok(())
    }

    /// Insert (or reactivate/update) a document record.
    pub fn insert_document(
        &self,
        collection: &str,
        path: &str,
        title: &str,
        hash: &str,
        created_at: &str,
        modified_at: &str,
    ) -> Result<(), String> {
        self.conn
            .execute(
                r"
            INSERT INTO documents (collection, path, title, hash, created_at, modified_at, active)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1)
            ON CONFLICT(collection, path) DO UPDATE SET
                title = excluded.title,
                hash = excluded.hash,
                modified_at = excluded.modified_at,
                active = 1
            ",
                params![collection, path, title, hash, created_at, modified_at],
            )
            .map_err(|e| format!("insert_document: {e}"))?;
        Ok(())
    }

    /// Find an active document by collection and path.
    pub fn find_active_document(
        &self,
        collection: &str,
        path: &str,
    ) -> Result<Option<(i64, String, String)>, String> {
        self.conn
            .query_row(
                "SELECT id, hash, title FROM documents WHERE collection = ?1 AND path = ?2 AND active = 1",
                params![collection, path],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(|e| format!("find_active_document: {e}"))
    }

    /// Deactivate a document (soft delete; the FTS trigger drops the row).
    pub fn deactivate_document(&self, collection: &str, path: &str) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE documents SET active = 0 WHERE collection = ?1 AND path = ?2",
                params![collection, path],
            )
            .map_err(|e| format!("deactivate_document: {e}"))?;
        Ok(())
    }

    /// All active document paths in a collection.
    pub fn get_active_document_paths(&self, collection: &str) -> Result<Vec<String>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT path FROM documents WHERE collection = ?1 AND active = 1")
            .map_err(|e| format!("get_active_document_paths: {e}"))?;
        let paths = stmt
            .query_map(params![collection], |row| row.get(0))
            .map_err(|e| format!("get_active_document_paths: {e}"))?
            .collect::<Result<Vec<String>, _>>()
            .map_err(|e| format!("get_active_document_paths: {e}"))?;
        Ok(paths)
    }

    /// Load one active document with its full body.
    pub fn get_document(
        &self,
        collection: &str,
        path: &str,
    ) -> Result<Option<DocumentResult>, String> {
        self.conn
            .query_row(
                r"
                SELECT
                    d.title,
                    d.hash,
                    d.modified_at,
                    c.doc,
                    LENGTH(c.doc) as body_length
                FROM documents d
                JOIN content c ON c.hash = d.hash
                WHERE d.collection = ?1 AND d.path = ?2 AND d.active = 1
                ",
                params![collection, path],
                |row| {
                    let title: String = row.get(0)?;
                    let hash: String = row.get(1)?;
                    let modified_at: String = row.get(2)?;
                    let body: String = row.get(3)?;

                    Ok(DocumentResult {
                        collection_name: collection.to_string(),
                        path: path.to_string(),
                        display_path: format!("{collection}/{path}"),
                        title,
                        hash,
                        modified_at,
                        body: Some(body),
                    })
                },
            )
            .optional()
            .map_err(|e| format!("get_document: {e}"))
    }

    /// Full-text search (BM25) over active documents, optionally restricted
    /// to one collection. Scores are negated bm25(): higher is better.
    pub fn search_fts(
        &self,
        query: &str,
        limit: usize,
        collection: Option<&str>,
    ) -> Result<Vec<SearchResult>, String> {
        let sql = if collection.is_some() {
            r"
            SELECT
                d.collection,
                d.path,
                d.title,
                d.hash,
                d.modified_at,
                bm25(documents_fts) as score
            FROM documents_fts fts
            JOIN documents d ON d.id = fts.rowid
            JOIN content c ON c.hash = d.hash
            WHERE documents_fts MATCH ?1
              AND d.collection = ?2
              AND d.active = 1
            ORDER BY score
            LIMIT ?3
            "
        } else {
            r"
            SELECT
                d.collection,
                d.path,
                d.title,
                d.hash,
                d.modified_at,
                bm25(documents_fts) as score
            FROM documents_fts fts
            JOIN documents d ON d.id = fts.rowid
            JOIN content c ON c.hash = d.hash
            WHERE documents_fts MATCH ?1
              AND d.active = 1
            ORDER BY score
            LIMIT ?2
            "
        };

        let mut stmt = self
            .conn
            .prepare(sql)
            .map_err(|e| format!("search_fts prepare: {e}"))?;

        let map_row = |row: &rusqlite::Row| -> rusqlite::Result<SearchResult> {
            let collection_name: String = row.get(0)?;
            let path: String = row.get(1)?;
            let title: String = row.get(2)?;
            let hash: String = row.get(3)?;
            let modified_at: String = row.get(4)?;
            let score: f64 = row.get(5)?;

            Ok(SearchResult {
                doc: DocumentResult {
                    collection_name: collection_name.clone(),
                    display_path: format!("{collection_name}/{path}"),
                    path: path.clone(),
                    title,
                    hash,
                    modified_at,
                    body: None,
                },
                // BM25 returns negative scores; negate so higher is better.
                score: -score,
            })
        };

        let results: Vec<SearchResult> = if let Some(coll) = collection {
            stmt.query_map(params![query, coll, limit as i64], map_row)
        } else {
            stmt.query_map(params![query, limit as i64], map_row)
        }
        .map_err(|e| format!("search_fts: {e}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("search_fts: {e}"))?;

        Ok(results)
    }

    /// Create the vector blob table (only when embeddings are enabled).
    /// Dimensions are not enforced in the schema; rows carry whatever the
    /// configured backend produced.
    pub fn ensure_vector_table(&self, _dimensions: usize) -> Result<(), String> {
        self.conn
            .execute(
                r"
                CREATE TABLE IF NOT EXISTS vectors_vec (
                    hash_seq TEXT PRIMARY KEY,
                    embedding BLOB NOT NULL
                )
                ",
                [],
            )
            .map_err(|e| format!("ensure_vector_table: {e}"))?;
        Ok(())
    }

    /// Insert (or replace) an embedding for one chunk of a content hash.
    /// `hash_seq` is `{hash}_{seq}` — the key format `vector_search.rs`
    /// reads, so it must not change.
    pub fn insert_embedding(
        &self,
        hash: &str,
        seq: usize,
        pos: usize,
        embedding: &[f32],
        model: &str,
        embedded_at: &str,
    ) -> Result<(), String> {
        self.conn
            .execute(
                r"
            INSERT OR REPLACE INTO content_vectors (hash, seq, pos, model, embedded_at)
            VALUES (?1, ?2, ?3, ?4, ?5)
            ",
                params![hash, seq as i64, pos as i64, model, embedded_at],
            )
            .map_err(|e| format!("insert_embedding metadata: {e}"))?;

        let hash_seq = format!("{hash}_{seq}");
        let embedding_bytes: Vec<u8> = embedding.iter().flat_map(|f| f.to_le_bytes()).collect();

        self.conn
            .execute(
                "INSERT OR REPLACE INTO vectors_vec (hash_seq, embedding) VALUES (?1, ?2)",
                params![hash_seq, embedding_bytes],
            )
            .map_err(|e| format!("insert_embedding blob: {e}"))?;

        Ok(())
    }

    /// Active documents with no `seq = 0` vector row yet: (hash, path, body).
    pub fn get_hashes_needing_embedding(&self) -> Result<Vec<(String, String, String)>, String> {
        let mut stmt = self
            .conn
            .prepare(
                r"
            SELECT DISTINCT d.hash, d.path, c.doc
            FROM documents d
            JOIN content c ON c.hash = d.hash
            LEFT JOIN content_vectors v ON d.hash = v.hash AND v.seq = 0
            WHERE d.active = 1 AND v.hash IS NULL
            ",
            )
            .map_err(|e| format!("get_hashes_needing_embedding: {e}"))?;

        let results = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .map_err(|e| format!("get_hashes_needing_embedding: {e}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("get_hashes_needing_embedding: {e}"))?;

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("memory.db")).unwrap();
        (store, dir)
    }

    #[test]
    fn hash_is_sha256_hex() {
        // Same input qmd hashed for every existing memory.db row.
        let h = Store::hash_content("hello");
        assert_eq!(
            h,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn roundtrip_document_and_fts() {
        let (store, _dir) = temp_store();
        let body = "# Title One\n\nthe quick brown fox jumps over the lazy dog";
        let hash = Store::hash_content(body);

        store.insert_content(&hash, body, "now").unwrap();
        store
            .insert_document("memory", "a.md", "Title One", &hash, "now", "now")
            .unwrap();

        let found = store.find_active_document("memory", "a.md").unwrap();
        assert_eq!(found.unwrap().1, hash);

        let hits = store.search_fts("\"fox\"", 10, None).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].doc.path, "a.md");
        assert!(hits[0].score > 0.0);

        // Collection filter excludes other collections.
        let brain_hits = store.search_fts("\"fox\"", 10, Some("brain")).unwrap();
        assert!(brain_hits.is_empty());

        let doc = store.get_document("memory", "a.md").unwrap().unwrap();
        assert_eq!(doc.body.as_deref(), Some(body));
    }

    #[test]
    fn deactivate_removes_from_fts() {
        let (store, _dir) = temp_store();
        let body = "uniqueword for deactivation test";
        let hash = Store::hash_content(body);
        store.insert_content(&hash, body, "now").unwrap();
        store
            .insert_document("memory", "b.md", "", &hash, "now", "now")
            .unwrap();
        assert_eq!(store.search_fts("\"uniqueword\"", 10, None).unwrap().len(), 1);

        store.deactivate_document("memory", "b.md").unwrap();
        assert_eq!(store.search_fts("\"uniqueword\"", 10, None).unwrap().len(), 0);
        assert!(store.find_active_document("memory", "b.md").unwrap().is_none());
    }

    #[test]
    fn embedding_roundtrip_and_backfill_list() {
        let (store, _dir) = temp_store();
        let body = "embed me";
        let hash = Store::hash_content(body);
        store.insert_content(&hash, body, "now").unwrap();
        store
            .insert_document("memory", "c.md", "", &hash, "now", "now")
            .unwrap();

        // Needs embedding: no seq-0 row yet.
        let needing = store.get_hashes_needing_embedding().unwrap();
        assert_eq!(needing.len(), 1);
        assert_eq!(needing[0].0, hash);

        store.ensure_vector_table(4).unwrap();
        store
            .insert_embedding(&hash, 0, 0, &[0.1, 0.2, 0.3, 0.4], "test-model", "now")
            .unwrap();

        assert!(store.get_hashes_needing_embedding().unwrap().is_empty());
    }

    #[test]
    fn title_extraction() {
        assert_eq!(Store::extract_title("# Hello\nbody"), "Hello");
        assert_eq!(Store::extract_title("intro\n## Sub\nbody"), "Sub");
        assert_eq!(Store::extract_title("no heading here"), "");
    }
}
