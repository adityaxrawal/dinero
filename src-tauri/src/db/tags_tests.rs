#[cfg(test)]
mod tests {
    use crate::db::tags::{
        delete, delete_transaction_tag, insert, insert_transaction_tag, select_all, select_by_id,
        select_by_transaction_id, update, TagsRow, TransactionTagsRow,
    };
    use chrono::Utc;
    use rusqlite::Connection;

    fn setup_db() -> Connection {
        crate::db::test_helpers::setup_test_db()
    }

    #[test]
    fn test_tags_crud() {
        let conn = setup_db();

        let tag = TagsRow {
            id: "tag_1".to_string(),
            name: "Groceries".to_string(),
            color_hex: Some("#ff0000".to_string()),
            created_at: Some(Utc::now().naive_utc()),
        };

        // 1. Insert
        insert(&conn, &tag).expect("Failed to insert tag");

        // 2. Select By ID
        let fetched = select_by_id(&conn, "tag_1").unwrap().unwrap();
        assert_eq!(fetched.name, "Groceries");
        assert_eq!(fetched.color_hex.unwrap(), "#ff0000");

        // 3. Update
        let updated_tag = TagsRow {
            name: "Supermarket".to_string(),
            color_hex: Some("#00ff00".to_string()),
            ..tag.clone()
        };
        update(&conn, &updated_tag).expect("Failed to update tag");

        let fetched_updated = select_by_id(&conn, "tag_1").unwrap().unwrap();
        assert_eq!(fetched_updated.name, "Supermarket");
        assert_eq!(fetched_updated.color_hex.unwrap(), "#00ff00");

        // 4. Select All
        let tag2 = TagsRow {
            id: "tag_2".to_string(),
            name: "Apple".to_string(),
            color_hex: None,
            created_at: Some(Utc::now().naive_utc()),
        };
        insert(&conn, &tag2).expect("Failed to insert tag2");

        let all_tags = select_all(&conn).unwrap();
        // Includes seed tags maybe? No, let's assume it doesn't or we check length >= 2
        // Actually, migrations seed categories and tags. Let's just find our tags.
        let tag_app = all_tags.iter().find(|t| t.name == "Apple").unwrap();
        assert_eq!(tag_app.id, "tag_2");

        let tag_sup = all_tags.iter().find(|t| t.name == "Supermarket").unwrap();
        assert_eq!(tag_sup.id, "tag_1");

        // 5. Delete
        delete(&conn, "tag_1").expect("Failed to delete tag");
        let after_delete = select_by_id(&conn, "tag_1").unwrap();
        assert!(after_delete.is_none());
    }

    #[test]
    fn test_transaction_tags() {
        let conn = setup_db();

        // Setup: We need a transaction to attach tags to.
        conn.execute(
            "INSERT INTO instruments (id, type, issuer_name, masked_identifier, status) VALUES ('inst_1', 'credit_card', 'Bank', '1234', 'active')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO transactions (id, instrument_id) VALUES ('tx_1', 'inst_1')",
            [],
        )
        .unwrap();

        let tag = TagsRow {
            id: "tag_1".to_string(),
            name: "Important".to_string(),
            color_hex: None,
            created_at: Some(Utc::now().naive_utc()),
        };
        insert(&conn, &tag).unwrap();

        let tx_tag = TransactionTagsRow {
            transaction_id: "tx_1".to_string(),
            tag_id: "tag_1".to_string(),
            created_at: Some(Utc::now().naive_utc()),
        };

        // 1. Insert Transaction Tag
        insert_transaction_tag(&conn, &tx_tag).expect("Failed to insert transaction tag");

        // 2. Select By Transaction ID
        let tags_for_tx = select_by_transaction_id(&conn, "tx_1").unwrap();
        assert_eq!(tags_for_tx.len(), 1);
        assert_eq!(tags_for_tx[0].tag_id, "tag_1");

        // 3. Delete Transaction Tag
        delete_transaction_tag(&conn, "tx_1", "tag_1").expect("Failed to delete transaction tag");
        let tags_after_delete = select_by_transaction_id(&conn, "tx_1").unwrap();
        assert_eq!(tags_after_delete.len(), 0);
    }

    /// Doc 30 TASK-API-007: `tags_delete` must not leave a dangling
    /// `transaction_tags` join row (`tag_id REFERENCES tags(id)`, no `ON
    /// DELETE CASCADE`) -- deleting a tag still linked to a transaction
    /// must clean up the join row rather than raising a foreign key
    /// violation or leaving orphaned data.
    #[test]
    fn test_delete_tag_still_linked_to_transaction() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO instruments (id, type, issuer_name, masked_identifier, status) VALUES ('inst_1', 'credit_card', 'Bank', '1234', 'active')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO transactions (id, instrument_id) VALUES ('tx_1', 'inst_1')",
            [],
        )
        .unwrap();
        let tag = TagsRow {
            id: "tag_linked".to_string(),
            name: "Linked".to_string(),
            color_hex: None,
            created_at: Some(Utc::now().naive_utc()),
        };
        insert(&conn, &tag).unwrap();
        insert_transaction_tag(
            &conn,
            &TransactionTagsRow {
                transaction_id: "tx_1".to_string(),
                tag_id: "tag_linked".to_string(),
                created_at: Some(Utc::now().naive_utc()),
            },
        )
        .unwrap();

        delete(&conn, "tag_linked").expect("delete must clean up the join row, not fail");

        assert!(select_by_id(&conn, "tag_linked").unwrap().is_none());
        assert_eq!(select_by_transaction_id(&conn, "tx_1").unwrap().len(), 0);
    }
}
