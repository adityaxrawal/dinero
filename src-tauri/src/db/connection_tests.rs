#[cfg(test)]
mod tests {
    use crate::db::crypto;
    use crate::db::init_db;
    use rusqlite::Connection;
    use std::fs;

    #[test]
    fn test_hardware_uuid_retrieval() {
        // This test simply ensures the ioreg command works on macOS and returns something.
        // It might fail in environments like GitHub Actions if they don't have ioreg,
        // but for a macOS desktop app, this verifies the core dependency.
        let result = crypto::get_hardware_uuid();
        assert!(result.is_ok(), "Should retrieve a hardware UUID on macOS");
    }

    #[tokio::test]
    async fn test_db_encryption_unreadable_without_key() {
        // Create a temporary directory for this test
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();

        let db_path = temp_dir.join("test_encrypted.db");

        // 1. Initialize the DB via our actual method, which creates it and encrypts it
        let pool = init_db(db_path.clone())
            .await
            .expect("Failed to initialize encrypted database");

        // Let's close the pool connection by dropping the pool so we know all locks are released
        drop(pool);

        // 2. Try to open the database file using standard rusqlite WITHOUT a key
        let conn_result = Connection::open(&db_path);
        assert!(
            conn_result.is_ok(),
            "Opening the file itself should succeed (SQLite doesn't check until read)"
        );
        let conn = conn_result.unwrap();

        // 3. Attempt to read from it. Since it's encrypted ciphertext, SQLite should fail to parse it
        let query_result: rusqlite::Result<i64> =
            conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0));

        match query_result {
            Err(rusqlite::Error::SqliteFailure(_err, Some(msg))) => {
                // The expected error message is "file is not a database" because the header is encrypted
                assert!(
                    msg.contains("file is not a database"),
                    "Expected 'file is not a database', got: {}",
                    msg
                );
            }
            Err(e) => panic!("Expected a specific SqliteFailure, got: {:?}", e),
            Ok(_) => panic!(
                "Read operation succeeded on an encrypted database without providing the key!"
            ),
        }

        // Cleanup
        let _ = fs::remove_dir_all(&temp_dir);
    }

    /// TASK-DB-001: `auto_vacuum = INCREMENTAL` must be set from the very
    /// first connection — DB-019's daily `PRAGMA incremental_vacuum`
    /// background task silently no-ops otherwise (auto_vacuum mode can't be
    /// changed to INCREMENTAL after tables already exist without a full
    /// `VACUUM`). SQLite reports auto_vacuum mode `2` for INCREMENTAL.
    #[tokio::test]
    async fn test_auto_vacuum_is_incremental() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("test_auto_vacuum.db");

        let pool = init_db(db_path.clone())
            .await
            .expect("Failed to initialize encrypted database");
        let conn = pool.get().await.expect("Failed to get pooled connection");
        let auto_vacuum: i64 = conn
            .interact(|c| c.query_row("PRAGMA auto_vacuum", [], |r| r.get(0)))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(auto_vacuum, 2, "auto_vacuum mode should be INCREMENTAL (2)");

        drop(pool);
        let _ = fs::remove_dir_all(&temp_dir);
    }
}
