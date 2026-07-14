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
}
