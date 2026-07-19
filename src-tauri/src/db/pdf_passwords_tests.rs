#[cfg(test)]
mod tests {
    use crate::db::pdf_passwords::{self, PdfPasswordsRow};
    use chrono::Utc;
    use rusqlite::Connection;

    fn setup_db() -> Connection {
        crate::db::test_helpers::setup_test_db()
    }

    #[test]
    fn test_pdf_passwords_crud_and_encryption_check() {
        let conn = setup_db();

        // Setup an instrument
        conn.execute(
            "INSERT INTO instruments (id, type, issuer_name, masked_identifier) VALUES (?1, ?2, ?3, ?4)",
            ["inst-1", "credit_card", "Test Issuer", "1234"],
        ).unwrap();

        let row = PdfPasswordsRow {
            id: "pw-1".to_string(),
            instrument_id: "inst-1".to_string(),
            password_ciphertext: "encrypted_blob_not_plaintext".to_string(),
            success_count: 0,
            last_used_at: None,
            created_at: Some(Utc::now().naive_utc()),
            updated_at: Some(Utc::now().naive_utc()),
        };

        // Ensure we don't accidentally store plaintext.
        assert_ne!(row.password_ciphertext, "my_super_secret_password");

        pdf_passwords::insert(&conn, &row).unwrap();

        let retrieved = pdf_passwords::select_by_instrument(&conn, "inst-1").unwrap();
        assert_eq!(retrieved.len(), 1);
        assert_eq!(retrieved[0].id, "pw-1");
        assert_eq!(
            retrieved[0].password_ciphertext,
            "encrypted_blob_not_plaintext"
        );

        pdf_passwords::increment_success(&conn, "pw-1").unwrap();

        let retrieved_again = pdf_passwords::select_by_instrument(&conn, "inst-1").unwrap();
        assert_eq!(retrieved_again[0].success_count, 1);
        assert!(retrieved_again[0].last_used_at.is_some());

        pdf_passwords::delete(&conn, "pw-1").unwrap();
        let retrieved_deleted = pdf_passwords::select_by_instrument(&conn, "inst-1").unwrap();
        assert!(retrieved_deleted.is_empty());
    }

    /// Doc 30 TASK-API-008 acceptance test: `settings_pdf_passwords_list`
    /// (Doc 19's real name for Doc 30's paraphrased
    /// `settings_list_pdf_password_hints`) is backed by
    /// `select_all_with_instrument`, whose `PdfPasswordSummary` return type
    /// has no ciphertext/plaintext field at all -- serializes the real
    /// stored ciphertext through the real function and asserts it never
    /// appears anywhere in the JSON the frontend would receive.
    #[test]
    fn test_pdf_password_hint_never_exposes_plaintext() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO instruments (id, type, issuer_name, masked_identifier) VALUES (?1, ?2, ?3, ?4)",
            ["inst-2", "credit_card", "Hint Test Issuer", "5678"],
        ).unwrap();
        let secret_ciphertext = "super-secret-ciphertext-blob-must-never-leak";
        pdf_passwords::insert(
            &conn,
            &PdfPasswordsRow {
                id: "pw-hint".to_string(),
                instrument_id: "inst-2".to_string(),
                password_ciphertext: secret_ciphertext.to_string(),
                success_count: 3,
                last_used_at: None,
                created_at: Some(Utc::now().naive_utc()),
                updated_at: Some(Utc::now().naive_utc()),
            },
        )
        .unwrap();

        let summaries = pdf_passwords::select_all_with_instrument(&conn).unwrap();
        let hint = summaries
            .iter()
            .find(|s| s.id == "pw-hint")
            .expect("summary must be present");
        assert_eq!(hint.issuer_name, "Hint Test Issuer");
        assert_eq!(hint.success_count, 3);

        let json = serde_json::to_string(&summaries).unwrap();
        assert!(
            !json.contains(secret_ciphertext),
            "the ciphertext must never appear in the settings-facing JSON response"
        );
    }
}
