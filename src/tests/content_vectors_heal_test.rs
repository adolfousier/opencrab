//! Regression tests for the `chunk_hash` column heal (#14).
//!
//! `CREATE TABLE IF NOT EXISTS` never amends a table an older build already
//! created, so every memory.db whose `content_vectors` predates the
//! `chunk_hash` column (#1107, 2026-08-19) kept the old schema forever — and
//! every `chunk_needs_embedding` call failed with "no such column" (#14),
//! killing the backfill cache check and spamming WARNs. The heal in
//! `Store::initialize` adds the column on open; these tests pin that
//! behavior on a database created with the pre-#1107 schema.

use crate::memory::db::Store;
use rusqlite::Connection;

/// Create a memory.db with the PRE-#1107 `content_vectors` schema (no
/// `chunk_hash` column) holding one legacy embedded-chunk row.
fn create_legacy_db(path: &std::path::Path) {
    let conn = Connection::open(path).expect("open legacy db");
    conn.execute_batch(
        r"
        CREATE TABLE content (
            hash TEXT PRIMARY KEY,
            doc TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        CREATE TABLE documents (
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
        CREATE TABLE content_vectors (
            hash TEXT NOT NULL,
            seq INTEGER NOT NULL DEFAULT 0,
            pos INTEGER NOT NULL DEFAULT 0,
            model TEXT NOT NULL,
            embedded_at TEXT NOT NULL,
            PRIMARY KEY (hash, seq)
        );
        INSERT INTO content (hash, doc, created_at)
            VALUES ('dochash', 'doc body', '2026-08-01T00:00:00Z');
        INSERT INTO content_vectors (hash, seq, pos, model, embedded_at)
            VALUES ('dochash', 0, 0, 'test-model', '2026-08-01T00:00:00Z');
        ",
    )
    .expect("create legacy schema");
}

/// True when the `content_vectors` table on disk has a `chunk_hash` column.
/// Reads through a separate connection so the check is independent of the
/// store's own handle.
fn has_chunk_hash_column(db_path: &std::path::Path) -> bool {
    let conn = Connection::open(db_path).expect("reopen db for schema probe");
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('content_vectors')
             WHERE name = 'chunk_hash'",
            [],
            |row| row.get(0),
        )
        .expect("probe content_vectors schema");
    count > 0
}

/// Opening a pre-#1107 store adds the missing column, keeps the rows, and
/// makes `chunk_needs_embedding` work again.
#[test]
fn heals_legacy_database_and_preserves_rows() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("legacy.db");
    create_legacy_db(&db_path);

    let store = Store::open(&db_path).expect("open heals the legacy store");

    assert!(
        has_chunk_hash_column(&db_path),
        "chunk_hash column must exist after open"
    );

    // The legacy row survived the ALTER.
    let conn = Connection::open(&db_path).unwrap();
    let rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM content_vectors", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 1, "legacy content_vectors rows must be preserved");
    drop(conn);

    // The cache check works again: NULL chunk_hash on the legacy row means
    // re-embed (true), a fresh hash for a new chunk means re-embed (true),
    // and an unchanged hash means skip (false).
    assert!(
        store
            .chunk_needs_embedding("dochash", 0, "any")
            .expect("cache check on healed store"),
        "legacy row with NULL chunk_hash must be re-embedded"
    );
    store
        .ensure_vector_table(384)
        .expect("vector table on healed store");
    store
        .insert_embedding(
            "dochash",
            1,
            0,
            &[0.0, 1.0],
            "test-model",
            "2026-08-01T00:00:00Z",
            Some("c1"),
        )
        .expect("insert_embedding on healed store");
    assert!(
        !store
            .chunk_needs_embedding("dochash", 1, "c1")
            .expect("cache check after insert"),
        "unchanged chunk must be skipped"
    );
    assert!(
        store
            .chunk_needs_embedding("dochash", 1, "c2")
            .expect("cache check after content change"),
        "changed chunk must be re-embedded"
    );
}

/// The heal is idempotent: reopening an already-healed store must not fail
/// (a duplicate ALTER would raise "duplicate column name").
#[test]
fn heal_is_idempotent_across_reopens() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("legacy.db");
    create_legacy_db(&db_path);

    Store::open(&db_path).expect("first open heals");
    assert!(has_chunk_hash_column(&db_path));

    let reopened = Store::open(&db_path).expect("second open must stay clean");
    assert!(has_chunk_hash_column(&db_path));
    reopened
        .chunk_needs_embedding("dochash", 0, "any")
        .expect("cache check on reopened store");
}

/// A brand-new store has the column straight from the DDL — the heal probe
/// runs and adds nothing.
#[test]
fn fresh_database_has_chunk_hash_from_ddl() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("fresh.db");

    let store = Store::open(&db_path).expect("open fresh store");
    assert!(has_chunk_hash_column(&db_path));
    assert!(
        store
            .chunk_needs_embedding("h", 0, "c")
            .expect("cache check on fresh store"),
        "no embedding exists yet on a fresh store"
    );
}
