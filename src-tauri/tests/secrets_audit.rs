//! TASK-AUTH-007: Secrets Management Audit (Integration Test Suite).
//!
//! Document 22 §26's final invariants #2 ("secrets never touch SQLite or
//! logs") and #4 ("IPC never returns raw secrets to the frontend"), audited
//! end-to-end against the crate's real public API rather than its
//! internals — these tests exercise `dinero_app_lib` the same way an
//! external consumer (or an attacker with only IPC access) would.

use dinero_app_lib::db;

fn temp_db_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("dinero_secrets_audit_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Recursively scans every regular file under `dir` for `needle` appearing
/// anywhere in its raw bytes. Binary-safe (checks bytes, not valid UTF-8) so
/// it also catches a key/token embedded inside a binary SQLite page.
fn any_file_contains(dir: &std::path::Path, needle: &[u8]) -> Option<std::path::PathBuf> {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(bytes) = std::fs::read(&path) {
                if bytes.windows(needle.len()).any(|w| w == needle) {
                    return Some(path);
                }
            }
        }
    }
    None
}

/// `test_sqlite_file_unreadable_without_keychain_key`: opening the encrypted
/// `finance.db` with plain `rusqlite` (no `PRAGMA key`) must fail — the
/// file is genuinely SQLCipher-encrypted on disk, not just access-controlled
/// by convention.
#[tokio::test]
async fn test_sqlite_file_unreadable_without_keychain_key() {
    let dir = temp_db_dir();
    let db_path = dir.join("finance.db");

    let pool = db::init_db(db_path.clone()).await;
    assert!(
        pool.is_ok(),
        "init_db should succeed with a real Keychain-derived key"
    );

    let raw_conn = rusqlite::Connection::open(&db_path).unwrap();
    let res = raw_conn.execute("SELECT count(*) FROM license_state", []);

    assert!(
        res.is_err(),
        "the database must be unreadable without PRAGMA key"
    );
    let err_str = res.unwrap_err().to_string();
    assert!(
        err_str.contains("file is not a database") || err_str.contains("encrypted"),
        "unexpected error (should indicate the file isn't a plain SQLite database): {}",
        err_str
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// `test_pdf_passwords_never_plaintext`: `encrypt_password`'s output must
/// never contain the plaintext as a substring, must never equal the
/// plaintext bytes, and must have exactly the byte-length AES-256-GCM
/// produces (12-byte nonce + ciphertext + 16-byte tag) — not, say, a
/// reversible encoding like base64 of the plaintext.
#[test]
fn test_pdf_passwords_never_plaintext() {
    use dinero_app_lib::statements::password::encrypt_password;

    let plaintext = "hunter2_super_secret_pdf_password";
    let blob = encrypt_password(plaintext).expect("encryption must succeed");

    assert_ne!(
        blob,
        plaintext.as_bytes(),
        "ciphertext must not equal the plaintext bytes"
    );
    assert!(
        !blob
            .windows(plaintext.len())
            .any(|w| w == plaintext.as_bytes()),
        "ciphertext must not contain the plaintext as a substring anywhere"
    );

    const NONCE_LEN: usize = 12;
    const GCM_TAG_LEN: usize = 16;
    assert_eq!(
        blob.len(),
        NONCE_LEN + plaintext.len() + GCM_TAG_LEN,
        "blob length must exactly match AES-256-GCM's nonce+ciphertext+tag structure, \
         ruling out a weaker or non-authenticated encoding"
    );
}

/// `test_sqlite_key_never_written_to_disk`: the derived SQLCipher key must
/// never appear, as a substring, in any file written to the app's data
/// directory during a real `init_db` — it exists only in process memory and
/// is handed to SQLCipher via an in-process `PRAGMA key` call, never
/// persisted anywhere itself.
#[tokio::test]
async fn test_sqlite_key_never_written_to_disk() {
    let dir = temp_db_dir();
    let db_path = dir.join("finance.db");

    let _pool = db::init_db(db_path.clone())
        .await
        .expect("init_db should succeed");

    let key = db::crypto::derive_database_key().expect("key derivation should succeed");
    assert!(
        key.len() > 16,
        "sanity check: derived key should be a substantial string, not trivially short"
    );

    if let Some(offender) = any_file_contains(&dir, key.as_bytes()) {
        panic!(
            "derived SQLCipher key must never be written to disk, but found it in {}",
            offender.display()
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}

/// `test_gmail_tokens_stored_only_in_keychain`: no table in the actual
/// schema has a column whose name suggests it holds a raw OAuth token or
/// secret, and `ConnectedAccountsRow` (the one struct written to
/// `connected_accounts`) is exhaustively listed field-by-field below — if a
/// future change ever added a token-shaped field to it, this test would
/// fail to *compile* (not just fail an assertion), forcing a conscious
/// decision about whether that field is safe.
#[tokio::test]
async fn test_gmail_tokens_stored_only_in_keychain() {
    let dir = temp_db_dir();
    let db_path = dir.join("finance.db");
    let pool = db::init_db(db_path.clone())
        .await
        .expect("init_db should succeed");
    let conn = pool.get().await.unwrap();

    // Exhaustive field list (no `..`) — a new field here that isn't also
    // added to this literal is a compile error, not a silent gap.
    let account = db::connected_accounts::ConnectedAccountsRow {
        id: "acc_audit".to_string(),
        profile_id: 1,
        email_address: Some("audit@example.com".to_string()),
        account_status: Some("ACTIVE".to_string()),
        last_history_id: None,
        created_at: None,
        updated_at: None,
    };

    // init_db's real migration/seed pipeline already seeds local_profile(id=1).
    conn.interact(move |c| {
        db::connected_accounts::insert_account(c, &account).unwrap();
    })
    .await
    .unwrap();

    // Scan every table's column names for anything token/secret-shaped.
    let suspicious_columns = conn
        .interact(|c| {
            let mut stmt = c
                .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
                .unwrap();
            let table_names: Vec<String> = stmt
                .query_map([], |r| r.get(0))
                .unwrap()
                .filter_map(|r| r.ok())
                .collect();

            let mut found = Vec::new();
            for table in table_names {
                let mut col_stmt = c.prepare(&format!("PRAGMA table_info({})", table)).unwrap();
                let columns: Vec<String> = col_stmt
                    .query_map([], |r| r.get::<_, String>(1))
                    .unwrap()
                    .filter_map(|r| r.ok())
                    .collect();
                for col in columns {
                    let lower = col.to_lowercase();
                    // `password_ciphertext` is deliberately exempt — its own
                    // name states it's ciphertext, and it's covered by
                    // test_pdf_passwords_never_plaintext instead.
                    // `secret_fields_masked` is a metadata column recording
                    // *which* fields were redacted before logging — it holds
                    // a list of field names, never a secret value itself.
                    if col == "password_ciphertext" || col == "secret_fields_masked" {
                        continue;
                    }
                    if lower.contains("access_token")
                        || lower.contains("refresh_token")
                        || lower == "token"
                        || lower.ends_with("_secret")
                        || lower.starts_with("secret_")
                        || lower == "secret"
                    {
                        found.push(format!("{}.{}", table, col));
                    }
                }
            }
            found
        })
        .await
        .unwrap();

    assert!(
        suspicious_columns.is_empty(),
        "found token/secret-shaped column(s) in the schema: {:?}",
        suspicious_columns
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// `test_tokens_never_returned_to_react`: no `#[tauri::command]` function's
/// signature (return type or argument types) mentions `TokenStore` or a
/// token-shaped identifier — the only place OAuth tokens are ever
/// constructed or read is `ingestion::oauth`'s private `save_token`/
/// `get_token`, neither of which is part of the IPC surface.
#[test]
fn test_tokens_never_returned_to_react() {
    let src_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut violations = Vec::new();

    fn scan(dir: &std::path::Path, violations: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                scan(&path, violations);
                continue;
            }
            if path.extension().map(|e| e == "rs") != Some(true) {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let lines: Vec<&str> = content.lines().collect();
            for (i, line) in lines.iter().enumerate() {
                if line.trim() == "#[tauri::command]" {
                    // The command's signature can span multiple lines
                    // (async fn ... over several args) before its closing
                    // `{` — join up to a reasonable window and check that
                    // text, not just the first line, so a wrapped signature
                    // doesn't slip past this check.
                    let sig: String = lines[i + 1..(i + 12).min(lines.len())].join(" ");
                    let sig_before_body = sig.split('{').next().unwrap_or(&sig);
                    if sig_before_body.contains("TokenStore")
                        || sig_before_body.contains("access_token")
                        || sig_before_body.contains("refresh_token")
                    {
                        violations.push(format!(
                            "{}:{}: command signature mentions a token-shaped type: {}",
                            path.display(),
                            i + 1,
                            sig_before_body.trim()
                        ));
                    }
                }
            }
        }
    }

    scan(&src_dir, &mut violations);
    assert!(
        violations.is_empty(),
        "IPC commands must never expose raw tokens: {:#?}",
        violations
    );
}

/// Sanity guard: none of the above should silently no-op due to a path
/// resolution mistake (e.g. `CARGO_MANIFEST_DIR` not pointing where
/// expected) — a directory that quietly doesn't exist would make
/// `test_tokens_never_returned_to_react` vacuously pass.
#[test]
fn src_dir_used_by_the_audit_actually_exists() {
    let src_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    assert!(src_dir.is_dir());
    assert!(src_dir.join("commands").is_dir());
}
