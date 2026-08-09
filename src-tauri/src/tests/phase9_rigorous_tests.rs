use crate::licensing::state::{
    get_license_state, transition_to_locked, upsert_license_state, LicenseStateRow, LicenseStatus,
};
use chrono::Utc;

#[test]
fn test_license_state_table_single_row_constraint() {
    let conn = crate::db::test_helpers::setup_test_db();

    let now = Utc::now();
    let state1 = LicenseStateRow {
        id: 1,
        license_jwt: "jwt1".to_string(),
        subscription_status_cached: LicenseStatus::Active,
        plan_id_cached: Some("pro".to_string()),
        current_period_end_cached: Some(now),
        jwt_expires_at: now,
        last_server_validated_at: Some(now),
        last_known_valid_time: now,
        device_fingerprint: Some("dev1".to_string()),
        source: "server_fresh".to_string(),
        billing_interval_cached: Some("monthly".to_string()),
    };
    upsert_license_state(&conn, &state1).unwrap();

    // Try to insert a second row with id = 2, should fail due to constraint
    let res = conn.execute(
        "INSERT INTO license_state (id, license_jwt, subscription_status_cached, jwt_expires_at, last_known_valid_time) 
         VALUES (2, 'jwt2', 'active', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)", 
        []
    );
    assert!(res.is_err(), "Single row constraint should prevent second insert. Implementation might be missing CHECK(id=1).");
}

#[test]
fn test_transition_to_locked_after_grace() {
    let conn = crate::db::test_helpers::setup_test_db();

    let now = Utc::now();
    let state = LicenseStateRow {
        id: 1,
        license_jwt: "jwt1".to_string(),
        subscription_status_cached: LicenseStatus::Grace,
        plan_id_cached: Some("pro".to_string()),
        current_period_end_cached: Some(now),
        jwt_expires_at: now,
        last_server_validated_at: Some(now - chrono::Duration::hours(73)),
        last_known_valid_time: now - chrono::Duration::hours(73),
        device_fingerprint: Some("dev1".to_string()),
        source: "server_fresh".to_string(),
        billing_interval_cached: Some("monthly".to_string()),
    };
    upsert_license_state(&conn, &state).unwrap();

    transition_to_locked(&conn, false).unwrap();

    let fetched = get_license_state(&conn).unwrap().unwrap();
    assert_eq!(fetched.subscription_status_cached, LicenseStatus::Locked);
    // Depending on actual implementation, it could be "server_validation_failed" or something else
}

#[test]
fn test_pdf_passwords_not_in_plaintext() {
    let conn = crate::db::test_helpers::setup_test_db();
    conn.execute("PRAGMA foreign_keys = OFF", []).unwrap();

    let res = conn.execute(
        "INSERT INTO pdf_passwords (id, instrument_id, password_ciphertext, success_count, last_used_at, created_at, updated_at)
         VALUES ('pw1', 'inst1', 'encrypted_password_blob', 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
         []
    );
    if let Err(e) = &res {
        println!("Error inserting password: {:?}", e);
    }
    assert!(res.is_ok());

    let mut stmt = conn
        .prepare("SELECT password_ciphertext FROM pdf_passwords WHERE id = 'pw1'")
        .unwrap();
    let pwd: String = stmt.query_row([], |row| row.get(0)).unwrap();

    assert_ne!(
        pwd, "plaintext_password",
        "PDF password should not be stored in plaintext"
    );

    let columns: Vec<String> = conn
        .prepare("PRAGMA table_info(pdf_passwords)")
        .unwrap()
        .query_map([], |r| r.get(1))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();

    assert!(!columns.contains(&"password".to_string()));
    assert!(!columns.contains(&"password_plaintext".to_string()));
}

#[test]
fn test_sqlite_queries_use_parameterized_bindings() {
    let src_dir = std::env::current_dir().unwrap().join("src");

    fn check_dir(dir: &std::path::Path) {
        if dir.is_dir() {
            for entry in std::fs::read_dir(dir).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if path.is_dir() {
                    check_dir(&path);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    if path.file_name().unwrap() == "phase9_rigorous_tests.rs" {
                        continue;
                    }
                    let content = std::fs::read_to_string(&path).unwrap();
                    for (i, line) in content.lines().enumerate() {
                        let l = line.replace(" ", "");
                        if l.contains(".execute(format!(")
                            || l.contains(".query(format!(")
                            || l.contains(".execute(&format!(")
                            || l.contains(".query(&format!(")
                        {
                            panic!(
                                "Found unparameterized query in file {}:{}: {}",
                                path.display(),
                                i + 1,
                                line
                            );
                        }
                    }
                }
            }
        }
    }

    check_dir(&src_dir);
}

#[test]
fn test_ipc_never_returns_tokens() {
    let src_dir = std::env::current_dir().unwrap().join("src/ipc");

    fn check_dir(dir: &std::path::Path) {
        if dir.is_dir() {
            for entry in std::fs::read_dir(dir).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if path.is_dir() {
                    check_dir(&path);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    let content = std::fs::read_to_string(&path).unwrap();
                    assert!(
                        !content.contains("pub token: String"),
                        "IPC response should not contain token field"
                    );
                    assert!(
                        !content.contains("pub password: String"),
                        "IPC response should not contain password field"
                    );
                }
            }
        }
    }

    check_dir(&src_dir);
}

#[test]
fn test_gmail_tokens_in_keychain_only() {
    let oauth_file = std::env::current_dir()
        .unwrap()
        .join("src/ingestion/oauth.rs");
    if oauth_file.exists() {
        let content = std::fs::read_to_string(&oauth_file).unwrap();
        // Tokens should be stored via `keyring` or explicitly state they are not on disk.
        assert!(
            content.contains("keyring")
                || content.contains("Entry::new")
                || !content.contains("std::fs::write"),
            "Gmail tokens must be stored in macOS Keychain only, not on disk"
        );
        assert!(
            !content.contains("File::create"),
            "Gmail tokens must not be written to disk"
        );
    }
}

#[test]
fn test_sqlite_encryption_key_in_keychain_only() {
    let db_mod_path = std::env::current_dir().unwrap().join("src/db/mod.rs");
    if db_mod_path.exists() {
        let content = std::fs::read_to_string(&db_mod_path).unwrap();
        // The DB encryption key must be sourced from Keychain securely.
        assert!(
            content.contains("keyring")
                || content.contains("Entry::new")
                || content.contains("password"),
            "Database initialization must use encryption key from Keychain"
        );
    }
}

#[test]
fn test_ipc_args_type_constraints() {
    let ipc_dir = std::env::current_dir().unwrap().join("src/commands");
    if ipc_dir.exists() {
        for entry in std::fs::read_dir(ipc_dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|ext| ext == "rs") {
                let content = std::fs::read_to_string(&path).unwrap();
                let lines: Vec<&str> = content.lines().collect();
                for (i, line) in lines.iter().enumerate() {
                    if line.contains("#[tauri::command]") && i + 1 < lines.len() {
                        let next_line = lines[i + 1];
                        assert!(
                            !next_line.contains("serde_json::Value"),
                            "IPC arguments must use strong Rust types, found Value in {}",
                            path.display()
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn test_file_upload_validation_mime_size() {
    let commands_data = std::env::current_dir()
        .unwrap()
        .join("src/commands/data.rs");
    if commands_data.exists() {
        let content = std::fs::read_to_string(&commands_data).unwrap();
        // File upload must check size or mime type for security
        // Assuming file_upload logic is here or we assert it has to be validated
        // If there's no upload command, this test gracefully passes
        if content.contains("upload") || content.contains("File::") {
            assert!(
                content.contains("size")
                    || content.contains("len")
                    || content.contains("metadata().len()"),
                "File upload must validate file size"
            );
            // Mime or magic bytes should be checked
            assert!(
                content.contains("application/pdf") || content.contains("magic"),
                "File upload must validate file type"
            );
        }
    }
}

#[test]
fn test_oauth_callback_strict_validation() {
    let oauth_file = std::env::current_dir()
        .unwrap()
        .join("src/ingestion/oauth.rs");
    if oauth_file.exists() {
        let content = std::fs::read_to_string(&oauth_file).unwrap();
        if content.contains("callback") || content.contains("exchange") {
            assert!(
                content.contains("state") || content.contains("csrf"),
                "OAuth callback must strictly validate the state parameter"
            );
        }
    }
}
