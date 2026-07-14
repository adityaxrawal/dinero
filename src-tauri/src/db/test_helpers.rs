//! TASK-DB-002. Shared test-fixture helper, replacing the ~19 files' local
//! `setup_db()`/`setup_test_db()` copies that used to do
//! `Connection::open_in_memory()` + the old synchronous
//! `rusqlite_migration`-based migration call. sqlx's migration connection
//! is a separate connection stack from rusqlite and cannot reach an
//! in-memory database another connection opened — every test now needs a
//! real (temp) file path instead.
#![cfg(test)]

use rusqlite::Connection;
use std::path::PathBuf;

/// Creates a fresh temp-file SQLite database, migrated via the same
/// `sqlx::migrate!` path production uses, and returns a plain rusqlite
/// `Connection` open against it — a drop-in replacement for the old
/// `Connection::open_in_memory()` + local migration call.
///
/// Passes `None` for the SQLCipher key (unencrypted) — these are
/// schema/logic tests, not encryption tests (encryption itself is covered
/// by `db::connection_tests`), and skipping it avoids needing to re-apply
/// a `PRAGMA key` on every reused pooled connection in every caller.
///
/// The backing temp directory is intentionally never cleaned up here (no
/// `Drop` guard) — call sites only get a `Connection`, and adding cleanup
/// would need every one of dozens of call sites to also manage a `TempDir`
/// guard's lifetime. Each file is a few KB; this matches how the OS temp
/// directory is already used elsewhere in this codebase's tests and is
/// swept independently by the OS.
pub fn setup_test_db() -> Connection {
    let db_path = fresh_temp_db_path();
    tokio::runtime::Runtime::new()
        .expect("failed to create tokio runtime for test migration")
        .block_on(crate::db::migrations::run_migrations(&db_path, None))
        .expect("test database migration failed");
    Connection::open(&db_path).expect("failed to open migrated test database")
}

/// Async twin of `setup_test_db()`, for call sites that are themselves
/// already running inside a Tokio runtime (`#[tokio::test]` functions) —
/// `setup_test_db()`'s own `Runtime::new().block_on(..)` would panic there
/// ("Cannot start a runtime from within a runtime").
pub async fn setup_test_db_async() -> Connection {
    let db_path = fresh_temp_db_path();
    crate::db::migrations::run_migrations(&db_path, None)
        .await
        .expect("test database migration failed");
    Connection::open(&db_path).expect("failed to open migrated test database")
}

/// Returns a fresh, not-yet-existing temp file path suitable for a
/// migrated test database — used directly by callers that need the pool
/// variant (`deadpool_sqlite::Pool`) rather than a plain `Connection`.
pub fn fresh_temp_db_path() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("failed to create temp test directory");
    dir.join("test.db")
}
