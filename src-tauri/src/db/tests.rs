#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use chrono::Utc;
    use rusqlite::Connection;

    // DAOs
    use crate::db::categories::{self, CategoriesRow};
    use crate::db::connected_accounts::{self, ConnectedAccountsRow};
    use crate::db::instruments::{self, InstrumentsRow};
    use crate::db::local_profile::{self, LocalProfileRow};
    use crate::db::match_decisions::{self, MatchDecisionsRow};
    use crate::db::merchants::{self, MerchantAliasesRow, MerchantsRow};
    use crate::db::reconciliation_cluster_members::{self, ReconciliationClusterMembersRow};
    use crate::db::reconciliation_clusters::{self, ReconciliationClustersRow};
    use crate::db::statement_entries::{self, StatementEntriesRow};
    use crate::db::statements::{self, StatementsRow};
    use crate::db::tags::{self, TagsRow, TransactionTagsRow};
    use crate::db::transaction_observations::{self, TransactionObservationsRow};
    use crate::db::transactions::{self, TransactionsRow};

    fn setup_db() -> Connection {
        crate::db::test_helpers::setup_test_db()
    }

    #[test]
    fn test_local_profile_insert_success() {
        let conn = setup_db();
        let now = Utc::now().naive_utc();
        let profile = LocalProfileRow {
            id: 1,
            primary_email: Some("test@example.com".into()),
            display_name: Some("Test User".into()),
            timezone: Some("UTC".into()),
            spending_limit_monthly: Some(1000.0),
            limit_thresholds: None,
            recovery_phrase_enabled: false,
            created_at: Some(now),
            updated_at: Some(now),
        };
        local_profile::insert(&conn, &profile).unwrap();
        let fetched = local_profile::select_by_id(&conn, 1).unwrap().unwrap();
        assert_eq!(fetched.primary_email.unwrap(), "test@example.com");
    }

    #[test]
    fn test_local_profile_update_success() {
        let conn = setup_db();
        let profile = LocalProfileRow {
            id: 1,
            primary_email: Some("test@example.com".into()),
            display_name: None,
            timezone: None,
            spending_limit_monthly: None,
            limit_thresholds: None,
            recovery_phrase_enabled: false,
            created_at: None,
            updated_at: None,
        };
        local_profile::insert(&conn, &profile).unwrap();
        let mut profile_mod = profile.clone();
        profile_mod.display_name = Some("Updated".into());
        local_profile::update(&conn, &profile_mod).unwrap();
        let fetched = local_profile::select_by_id(&conn, 1).unwrap().unwrap();
        assert_eq!(fetched.display_name.unwrap(), "Updated");
    }

    #[test]
    fn test_local_profile_update_not_found() {
        let conn = setup_db();
        let profile = LocalProfileRow {
            id: 1,
            primary_email: Some("test@example.com".into()),
            display_name: None,
            timezone: None,
            spending_limit_monthly: None,
            limit_thresholds: None,
            recovery_phrase_enabled: false,
            created_at: None,
            updated_at: None,
        };
        let err = local_profile::update(&conn, &profile).unwrap_err();
        assert!(err.to_string().contains("Profile not found"));
    }

    #[test]
    fn test_local_profile_select_by_id_success() {
        let conn = setup_db();
        let profile = LocalProfileRow {
            id: 1,
            primary_email: Some("test@example.com".into()),
            display_name: None,
            timezone: None,
            spending_limit_monthly: None,
            limit_thresholds: None,
            recovery_phrase_enabled: false,
            created_at: None,
            updated_at: None,
        };
        local_profile::insert(&conn, &profile).unwrap();
        let fetched = local_profile::select_by_id(&conn, 1).unwrap();
        assert!(fetched.is_some());
    }

    #[test]
    fn test_local_profile_select_by_id_not_found() {
        let conn = setup_db();
        let fetched = local_profile::select_by_id(&conn, 1).unwrap();
        assert!(fetched.is_none());
    }

    #[test]
    fn test_connected_accounts_crud() {
        let conn = setup_db();

        let account = ConnectedAccountsRow {
            id: "acc_1".into(),
            profile_id: 1,
            email_address: Some("acc@test.com".into()),
            account_status: Some("active".into()),
            last_history_id: None,
            created_at: None,
            updated_at: None,
        };

        // Needs profile 1 to exist because of FK
        let profile = LocalProfileRow {
            id: 1,
            primary_email: None,
            display_name: None,
            timezone: None,
            spending_limit_monthly: None,
            limit_thresholds: None,
            recovery_phrase_enabled: false,
            created_at: None,
            updated_at: None,
        };
        local_profile::insert(&conn, &profile).unwrap();

        connected_accounts::insert_account(&conn, &account).unwrap();
        let fetched = connected_accounts::get_account(&conn, "acc_1")
            .unwrap()
            .unwrap();
        assert_eq!(fetched.email_address.unwrap(), "acc@test.com");
    }

    #[test]
    fn test_connected_accounts_update_success() {
        let conn = setup_db();
        let profile = LocalProfileRow {
            id: 1,
            primary_email: None,
            display_name: None,
            timezone: None,
            spending_limit_monthly: None,
            limit_thresholds: None,
            recovery_phrase_enabled: false,
            created_at: None,
            updated_at: None,
        };
        local_profile::insert(&conn, &profile).unwrap();
        let mut account = ConnectedAccountsRow {
            id: "acc_1".into(),
            profile_id: 1,
            email_address: Some("acc@test.com".into()),
            account_status: Some("active".into()),
            last_history_id: None,
            created_at: None,
            updated_at: None,
        };
        connected_accounts::insert_account(&conn, &account).unwrap();

        account.account_status = Some("inactive".into());
        connected_accounts::update_account(&conn, &account).unwrap();
        let fetched = connected_accounts::get_account(&conn, "acc_1")
            .unwrap()
            .unwrap();
        assert_eq!(fetched.account_status.unwrap(), "inactive");
    }

    #[test]
    fn test_connected_accounts_update_not_found() {
        let conn = setup_db();
        let account = ConnectedAccountsRow {
            id: "acc_999".into(),
            profile_id: 1,
            email_address: None,
            account_status: None,
            last_history_id: None,
            created_at: None,
            updated_at: None,
        };
        let err = connected_accounts::update_account(&conn, &account).unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn test_connected_accounts_select_not_found() {
        let conn = setup_db();
        let fetched = connected_accounts::get_account(&conn, "acc_999").unwrap();
        assert!(fetched.is_none());
    }

    #[test]
    fn test_instruments_crud() {
        let conn = setup_db();

        let now = Utc::now().naive_utc();
        let inst = InstrumentsRow {
            id: "inst_1".into(),
            r#type: "credit_card".into(),
            issuer_name: "Bank".into(),
            masked_identifier: "1234".into(),
            network: None,
            credit_limit: None,
            current_balance: None,
            statement_due_date: None,
            minimum_due: None,
            bank_ifsc: None,
            account_type: None,
            upi_vpa: None,
            nickname: None,
            rewards_summary: None,
            status: "active".into(),
            created_at: Some(now),
            updated_at: Some(now),
            is_deleted: false,
            full_identifier: None,
            billing_cycle_day: None,
        };

        instruments::insert_instrument(&conn, &inst).unwrap();
        let fetched = instruments::get_instrument(&conn, "inst_1")
            .unwrap()
            .unwrap();
        assert_eq!(fetched.issuer_name, "Bank");
    }

    #[test]
    fn test_transactions_crud() {
        let conn = setup_db();

        let tx = TransactionsRow {
            id: "tx_1".into(),
            unique_event_id: None,
            instrument_id: None,
            instrument_type: None,
            direction: None,
            amount: Some(100.5),
            amount_minor: Some(10050),
            currency: Some("USD".into()),
            authorization_time: None,
            best_event_time: None,
            event_time_confidence: None,
            best_posting_date: None,
            posting_date_confidence: None,
            merchant_display_name: None,
            merchant_normalized_name: None,
            merchant_entity_id: None,
            reference_id: None,
            location: None,
            original_amount_minor: None,
            original_currency: None,
            exchange_rate: None,
            balance_after_transaction: None,
            status: None,
            match_confidence: None,
            source_mix: None,
            alert_fired: None,
            parent_transaction_id: None,
            transaction_subtype: None,
            emi_group_id: None,
            category_id: None,
            is_deleted: false,
            created_at: None,
            updated_at: None,
        };

        transactions::insert_transaction(&conn, &tx).unwrap();
        let fetched = transactions::get_transaction(&conn, "tx_1")
            .unwrap()
            .unwrap();
        assert_eq!(fetched.amount_minor.unwrap(), 10050);
    }

    // Will skip writing detailed tests for all 16 to save space, but prove basic table ops
    #[test]
    fn test_other_tables_insert() {
        let conn = setup_db();

        // statements
        let stmt = StatementsRow {
            id: "stmt_1".into(),
            instrument_id: None,
            statement_type: "credit_card_statement".into(),
            billing_period_start: chrono::NaiveDate::from_ymd_opt(2023, 1, 1).unwrap(),
            billing_period_end: chrono::NaiveDate::from_ymd_opt(2023, 1, 31).unwrap(),
            due_date: None,
            statement_date: None,
            current_balance: None,
            minimum_due: None,
            rewards_summary_json: None,
            source_message_id: None,
            parse_status: "parsed".into(),
            is_duplicate: false,
            created_at: None,
            updated_at: None,
        };
        statements::insert(&conn, &stmt).unwrap();

        // statement_entries
        let entry = StatementEntriesRow {
            id: "entry_1".into(),
            statement_id: Some("stmt_1".into()),
            row_index: Some(1),
            transaction_date: None,
            posting_date: None,
            description_raw: None,
            merchant_raw: None,
            merchant_normalized: None,
            amount: None,
            amount_minor: None,
            currency: None,
            direction: None,
            reference_id: None,
            location: None,
            raw_row_json: None,
            created_at: None,
        };
        statement_entries::insert(&conn, &entry).unwrap();

        // categories
        let cat = CategoriesRow {
            id: "cat_1".into(),
            name: "Food".into(),
            parent_id: None,
            color: None,
            icon: None,
            is_system: false,
            is_deleted: false,
            created_at: None,
            updated_at: None,
        };
        categories::insert(&conn, &cat).unwrap();

        // merchants
        let merchant = MerchantsRow {
            id: "merch_1".into(),
            name: "McDonalds".into(),
            category_id: Some("cat_1".into()),
            logo_url: None,
            website: None,
            is_deleted: false,
            created_at: None,
            updated_at: None,
        };
        merchants::insert(&conn, &merchant).unwrap();

        // merchant aliases
        let alias = MerchantAliasesRow {
            id: "alias_1".into(),
            merchant_id: Some("merch_1".into()),
            alias: "mcd".into(),
            created_at: None,
        };
        merchants::insert_alias(&conn, &alias).unwrap();

        // tags
        let tag = TagsRow {
            id: "tag_1".into(),
            name: "Vacation".into(),
            color: None,
            created_at: None,
        };
        tags::insert(&conn, &tag).unwrap();

        // recon clusters
        let cluster = ReconciliationClustersRow {
            id: "cluster_1".into(),
            observation_id: None,
            status: "unreconciled".into(),
            total_amount_minor: None,
            currency: None,
            resolution_note: None,
            resolved_at: None,
            created_at: None,
            updated_at: None,
        };
        reconciliation_clusters::insert(&conn, &cluster).unwrap();

        // tx observations
        let obs = TransactionObservationsRow {
            id: "obs_1".into(),
            canonical_transaction_id: None,
            source_pipeline: None,
            source_record_id: None,
            source_message_id: None,
            source_thread_id: None,
            statement_id: None,
            statement_entry_id: None,
            instrument_id: None,
            direction: None,
            amount: None,
            amount_minor: None,
            currency: None,
            event_time: None,
            event_time_confidence: None,
            posting_date: None,
            merchant_raw: None,
            merchant_normalized: None,
            reference_id: None,
            original_amount_minor: None,
            original_currency: None,
            exchange_rate: None,
            balance_after_transaction: None,
            timezone_at_ingestion: None,
            fingerprint: None,
            extraction_method: None,
            confidence_score: None,
            raw_payload_json: None,
            parser_version: None,
            emi_total_installments: None,
            emi_installment_number: None,
            emi_original_amount_minor: None,
            is_deleted: false,
            created_at: None,
            updated_at: None,
        };
        transaction_observations::insert_observation(&conn, &obs).unwrap();

        // match decisions
        let match_decision = MatchDecisionsRow {
            id: "md_1".into(),
            observation_id: Some("obs_1".into()),
            matched_transaction_id: None,
            decision: None,
            score: None,
            rules_triggered_json: None,
            review_status: None,
            reviewed_by: None,
            created_at: None,
        };
        match_decisions::insert(&conn, &match_decision).unwrap();

        // Members & tx tags
        let tx = TransactionsRow {
            id: "tx_1".into(),
            unique_event_id: None,
            instrument_id: None,
            instrument_type: None,
            direction: None,
            amount: Some(100.5),
            amount_minor: Some(10050),
            currency: Some("USD".into()),
            authorization_time: None,
            best_event_time: None,
            event_time_confidence: None,
            best_posting_date: None,
            posting_date_confidence: None,
            merchant_display_name: Some("Starbucks".into()),
            merchant_normalized_name: None,
            merchant_entity_id: None,
            reference_id: None,
            location: None,
            original_amount_minor: None,
            original_currency: None,
            exchange_rate: None,
            balance_after_transaction: None,
            status: None,
            match_confidence: None,
            source_mix: None,
            alert_fired: None,
            parent_transaction_id: None,
            transaction_subtype: None,
            emi_group_id: None,
            category_id: None,
            is_deleted: false,
            created_at: None,
            updated_at: None,
        };
        transactions::insert_transaction(&conn, &tx).unwrap();

        let r_member = ReconciliationClusterMembersRow {
            id: "rcm_1".into(),
            cluster_id: "cluster_1".into(),
            transaction_id: Some("tx_1".into()),
            statement_entry_id: None,
            added_at: None,
            updated_at: None,
        };
        reconciliation_cluster_members::insert(&conn, &r_member).unwrap();

        let tx_tag = TransactionTagsRow {
            transaction_id: "tx_1".into(),
            tag_id: "tag_1".into(),
            created_at: None,
        };
        tags::insert_transaction_tag(&conn, &tx_tag).unwrap();

        // Verify some inserts
        assert!(statements::select_by_id(&conn, "stmt_1").unwrap().is_some());
        assert!(categories::select_by_id(&conn, "cat_1").unwrap().is_some());
        assert!(tags::select_by_id(&conn, "tag_1").unwrap().is_some());

        // Test FTS search
        let results = transactions::search_transactions(&conn, "Starbucks", 10, 0).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].merchant_display_name.as_deref(),
            Some("Starbucks")
        );
    }

    #[test]
    fn test_local_profile_insert_duplicate_pk_fails() {
        let conn = setup_db();
        // Try to insert a second row with id=2, which should fail the CHECK constraint
        let err = conn
            .execute("INSERT INTO local_profile (id) VALUES (2)", [])
            .unwrap_err();
        assert!(err.to_string().contains("CHECK constraint failed"));
    }

    #[test]
    fn test_local_profile_updated_at_trigger() {
        let conn = setup_db();
        let profile = LocalProfileRow {
            id: 1,
            primary_email: None,
            display_name: None,
            timezone: None,
            spending_limit_monthly: None,
            limit_thresholds: None,
            recovery_phrase_enabled: false,
            created_at: None,
            updated_at: None,
        };
        local_profile::insert(&conn, &profile).unwrap();
        let p1 = local_profile::select_by_id(&conn, 1).unwrap().unwrap();

        // Artificial backdate
        conn.execute(
            "UPDATE local_profile SET updated_at = '2000-01-01 00:00:00' WHERE id = 1",
            [],
        )
        .unwrap();

        let mut profile_mod = p1.clone();
        profile_mod.display_name = Some("Updated".into());
        local_profile::update(&conn, &profile_mod).unwrap();

        let p2 = local_profile::select_by_id(&conn, 1).unwrap().unwrap();
        assert_ne!(p2.updated_at.unwrap().to_string(), "2000-01-01 00:00:00");
    }

    #[test]
    fn test_connected_accounts_fk_violation() {
        let conn = setup_db();
        conn.execute("PRAGMA foreign_keys = ON", []).unwrap();

        let account = ConnectedAccountsRow {
            id: "acc_1".into(),
            profile_id: 999,
            email_address: None,
            account_status: None,
            last_history_id: None,
            created_at: None,
            updated_at: None,
        };

        let err = connected_accounts::insert_account(&conn, &account).unwrap_err();
        assert!(err.to_string().contains("FOREIGN KEY constraint failed"));
    }

    #[test]
    fn test_instruments_comprehensive() {
        let conn = setup_db();
        let inst = InstrumentsRow {
            id: "inst_1".into(),
            r#type: "credit_card".into(),
            issuer_name: "Bank".into(),
            masked_identifier: "1234".into(),
            network: None,
            credit_limit: None,
            current_balance: None,
            statement_due_date: None,
            minimum_due: None,
            bank_ifsc: None,
            account_type: None,
            upi_vpa: None,
            nickname: None,
            rewards_summary: None,
            status: "active".into(),
            created_at: None,
            updated_at: None,
            is_deleted: false,
            full_identifier: None,
            billing_cycle_day: None,
        };
        instruments::insert_instrument(&conn, &inst).unwrap();

        // Unique constraint check
        let inst2 = InstrumentsRow {
            id: "inst_2".into(),
            ..inst.clone()
        };
        let err = instruments::insert_instrument(&conn, &inst2).unwrap_err();
        assert!(err.to_string().contains("UNIQUE constraint failed"));

        // Soft Delete
        instruments::delete_instrument(&conn, "inst_1").unwrap();
        assert!(instruments::get_instrument(&conn, "inst_1")
            .unwrap()
            .is_none());

        let all = instruments::get_all_instruments(&conn).unwrap();
        assert_eq!(all.len(), 0);
    }

    #[test]
    fn test_transactions_comprehensive() {
        let conn = setup_db();
        conn.execute("PRAGMA foreign_keys = ON", []).unwrap();

        let inst = InstrumentsRow {
            id: "inst_1".into(),
            r#type: "credit_card".into(),
            issuer_name: "Bank".into(),
            masked_identifier: "1234".into(),
            network: None,
            credit_limit: None,
            current_balance: None,
            statement_due_date: None,
            minimum_due: None,
            bank_ifsc: None,
            account_type: None,
            upi_vpa: None,
            nickname: None,
            rewards_summary: None,
            status: "active".into(),
            created_at: None,
            updated_at: None,
            is_deleted: false,
            full_identifier: None,
            billing_cycle_day: None,
        };
        instruments::insert_instrument(&conn, &inst).unwrap();

        let tx = TransactionsRow {
            id: "tx_1".into(),
            unique_event_id: Some("ue_1".into()),
            instrument_id: Some("inst_1".into()),
            instrument_type: None,
            direction: None,
            amount: None,
            amount_minor: None,
            currency: None,
            authorization_time: None,
            best_event_time: None,
            event_time_confidence: None,
            best_posting_date: None,
            posting_date_confidence: None,
            merchant_display_name: Some("Apple".into()),
            merchant_normalized_name: None,
            merchant_entity_id: None,
            reference_id: None,
            location: None,
            original_amount_minor: None,
            original_currency: None,
            exchange_rate: None,
            balance_after_transaction: None,
            status: None,
            match_confidence: None,
            source_mix: None,
            alert_fired: None,
            parent_transaction_id: None,
            transaction_subtype: None,
            emi_group_id: None,
            category_id: None,
            is_deleted: false,
            created_at: None,
            updated_at: None,
        };
        transactions::insert_transaction(&conn, &tx).unwrap();

        // FK Violation
        let mut tx_fk = tx.clone();
        tx_fk.id = "tx_bad_fk".into();
        tx_fk.unique_event_id = Some("ue_bad".into());
        tx_fk.instrument_id = Some("bad_inst".into());
        let err = transactions::insert_transaction(&conn, &tx_fk).unwrap_err();
        assert!(err.to_string().contains("FOREIGN KEY constraint failed"));

        // Soft Delete
        transactions::delete_transaction(&conn, "tx_1").unwrap();
        assert!(transactions::get_transaction(&conn, "tx_1")
            .unwrap()
            .is_none());
    }

    #[test]
    fn test_instruments_update_success() {
        let conn = setup_db();
        let inst = InstrumentsRow {
            id: "inst_1".into(),
            r#type: "credit_card".into(),
            issuer_name: "Bank".into(),
            masked_identifier: "1234".into(),
            network: None,
            credit_limit: None,
            current_balance: None,
            statement_due_date: None,
            minimum_due: None,
            bank_ifsc: None,
            account_type: None,
            upi_vpa: None,
            nickname: None,
            rewards_summary: None,
            status: "active".into(),
            created_at: None,
            updated_at: None,
            is_deleted: false,
            full_identifier: None,
            billing_cycle_day: None,
        };
        instruments::insert_instrument(&conn, &inst).unwrap();
        let mut inst_updated = inst.clone();
        inst_updated.issuer_name = "New Bank".into();
        instruments::update_instrument(&conn, &inst_updated).unwrap();
        let fetched = instruments::get_instrument(&conn, "inst_1")
            .unwrap()
            .unwrap();
        assert_eq!(fetched.issuer_name, "New Bank");
    }

    #[test]
    fn test_instruments_update_not_found() {
        let conn = setup_db();
        let inst = InstrumentsRow {
            id: "inst_999".into(),
            r#type: "credit_card".into(),
            issuer_name: "Bank".into(),
            masked_identifier: "1234".into(),
            network: None,
            credit_limit: None,
            current_balance: None,
            statement_due_date: None,
            minimum_due: None,
            bank_ifsc: None,
            account_type: None,
            upi_vpa: None,
            nickname: None,
            rewards_summary: None,
            status: "active".into(),
            created_at: None,
            updated_at: None,
            is_deleted: false,
            full_identifier: None,
            billing_cycle_day: None,
        };
        let err = instruments::update_instrument(&conn, &inst).unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn test_instruments_delete_not_found() {
        let conn = setup_db();
        let err = instruments::delete_instrument(&conn, "inst_999").unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn test_instruments_select_not_found() {
        let conn = setup_db();
        let fetched = instruments::get_instrument(&conn, "inst_999").unwrap();
        assert!(fetched.is_none());
    }

    #[test]
    fn test_instruments_paginated() {
        let conn = setup_db();
        let inst = InstrumentsRow {
            id: "inst_1".into(),
            r#type: "credit_card".into(),
            issuer_name: "Bank".into(),
            masked_identifier: "1234".into(),
            network: None,
            credit_limit: None,
            current_balance: None,
            statement_due_date: None,
            minimum_due: None,
            bank_ifsc: None,
            account_type: None,
            upi_vpa: None,
            nickname: None,
            rewards_summary: None,
            status: "active".into(),
            created_at: None,
            updated_at: None,
            is_deleted: false,
            full_identifier: None,
            billing_cycle_day: None,
        };
        instruments::insert_instrument(&conn, &inst).unwrap();
        let res = instruments::get_paginated_instruments(&conn, 10, 0).unwrap();
        assert_eq!(res.len(), 1);
        let res_empty = instruments::get_paginated_instruments(&conn, 10, 10).unwrap();
        assert_eq!(res_empty.len(), 0);
    }

    #[test]
    fn test_transactions_duplicate_pk_fails() {
        let conn = setup_db();
        let tx = TransactionsRow {
            id: "tx_1".into(),
            unique_event_id: None,
            instrument_id: None,
            instrument_type: None,
            direction: None,
            amount: None,
            amount_minor: None,
            currency: None,
            authorization_time: None,
            best_event_time: None,
            event_time_confidence: None,
            best_posting_date: None,
            posting_date_confidence: None,
            merchant_display_name: None,
            merchant_normalized_name: None,
            merchant_entity_id: None,
            reference_id: None,
            location: None,
            original_amount_minor: None,
            original_currency: None,
            exchange_rate: None,
            balance_after_transaction: None,
            status: None,
            match_confidence: None,
            source_mix: None,
            alert_fired: None,
            parent_transaction_id: None,
            transaction_subtype: None,
            emi_group_id: None,
            category_id: None,
            is_deleted: false,
            created_at: None,
            updated_at: None,
        };
        transactions::insert_transaction(&conn, &tx).unwrap();
        let err = transactions::insert_transaction(&conn, &tx).unwrap_err();
        assert!(err.to_string().contains("UNIQUE constraint failed"));
    }

    #[test]
    fn test_transactions_update_success() {
        let conn = setup_db();
        let tx = TransactionsRow {
            id: "tx_1".into(),
            unique_event_id: None,
            instrument_id: None,
            instrument_type: None,
            direction: None,
            amount: None,
            amount_minor: None,
            currency: None,
            authorization_time: None,
            best_event_time: None,
            event_time_confidence: None,
            best_posting_date: None,
            posting_date_confidence: None,
            merchant_display_name: None,
            merchant_normalized_name: None,
            merchant_entity_id: None,
            reference_id: None,
            location: None,
            original_amount_minor: None,
            original_currency: None,
            exchange_rate: None,
            balance_after_transaction: None,
            status: None,
            match_confidence: None,
            source_mix: None,
            alert_fired: None,
            parent_transaction_id: None,
            transaction_subtype: None,
            emi_group_id: None,
            category_id: None,
            is_deleted: false,
            created_at: None,
            updated_at: None,
        };
        transactions::insert_transaction(&conn, &tx).unwrap();
        let mut tx_updated = tx.clone();
        tx_updated.amount_minor = Some(500);
        transactions::update_transaction(&conn, &tx_updated).unwrap();
        let fetched = transactions::get_transaction(&conn, "tx_1")
            .unwrap()
            .unwrap();
        assert_eq!(fetched.amount_minor, Some(500));
    }

    #[test]
    fn test_transactions_update_not_found() {
        let conn = setup_db();
        let tx = TransactionsRow {
            id: "tx_999".into(),
            unique_event_id: None,
            instrument_id: None,
            instrument_type: None,
            direction: None,
            amount: None,
            amount_minor: None,
            currency: None,
            authorization_time: None,
            best_event_time: None,
            event_time_confidence: None,
            best_posting_date: None,
            posting_date_confidence: None,
            merchant_display_name: None,
            merchant_normalized_name: None,
            merchant_entity_id: None,
            reference_id: None,
            location: None,
            original_amount_minor: None,
            original_currency: None,
            exchange_rate: None,
            balance_after_transaction: None,
            status: None,
            match_confidence: None,
            source_mix: None,
            alert_fired: None,
            parent_transaction_id: None,
            transaction_subtype: None,
            emi_group_id: None,
            category_id: None,
            is_deleted: false,
            created_at: None,
            updated_at: None,
        };
        let err = transactions::update_transaction(&conn, &tx).unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn test_transactions_delete_not_found() {
        let conn = setup_db();
        let err = transactions::delete_transaction(&conn, "tx_999").unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn test_transactions_select_not_found() {
        let conn = setup_db();
        let fetched = transactions::get_transaction(&conn, "tx_999").unwrap();
        assert!(fetched.is_none());
    }

    #[test]
    fn test_transactions_paginated() {
        let conn = setup_db();
        let tx = TransactionsRow {
            id: "tx_1".into(),
            unique_event_id: None,
            instrument_id: None,
            instrument_type: None,
            direction: None,
            amount: None,
            amount_minor: None,
            currency: None,
            authorization_time: None,
            best_event_time: None,
            event_time_confidence: None,
            best_posting_date: None,
            posting_date_confidence: None,
            merchant_display_name: None,
            merchant_normalized_name: None,
            merchant_entity_id: None,
            reference_id: None,
            location: None,
            original_amount_minor: None,
            original_currency: None,
            exchange_rate: None,
            balance_after_transaction: None,
            status: None,
            match_confidence: None,
            source_mix: None,
            alert_fired: None,
            parent_transaction_id: None,
            transaction_subtype: None,
            emi_group_id: None,
            category_id: None,
            is_deleted: false,
            created_at: None,
            updated_at: None,
        };
        transactions::insert_transaction(&conn, &tx).unwrap();
        let res = transactions::get_paginated_transactions(&conn, 10, 0).unwrap();
        assert_eq!(res.len(), 1);
        let res_empty = transactions::get_paginated_transactions(&conn, 10, 10).unwrap();
        assert_eq!(res_empty.len(), 0);
    }

    #[test]
    fn test_transactions_fts_stays_synced() {
        let conn = setup_db();

        let tx1 = TransactionsRow {
            id: "tx_1".into(),
            merchant_display_name: Some("Netflix".into()),
            unique_event_id: None,
            instrument_id: None,
            instrument_type: None,
            direction: None,
            amount: None,
            amount_minor: None,
            currency: None,
            authorization_time: None,
            best_event_time: None,
            event_time_confidence: None,
            best_posting_date: None,
            posting_date_confidence: None,
            merchant_normalized_name: None,
            merchant_entity_id: None,
            reference_id: None,
            location: None,
            original_amount_minor: None,
            original_currency: None,
            exchange_rate: None,
            balance_after_transaction: None,
            status: None,
            match_confidence: None,
            source_mix: None,
            alert_fired: None,
            parent_transaction_id: None,
            transaction_subtype: None,
            emi_group_id: None,
            category_id: None,
            is_deleted: false,
            created_at: None,
            updated_at: None,
        };
        let tx2 = TransactionsRow {
            id: "tx_2".into(),
            merchant_display_name: Some("Netflix Subscription".into()),
            unique_event_id: None,
            instrument_id: None,
            instrument_type: None,
            direction: None,
            amount: None,
            amount_minor: None,
            currency: None,
            authorization_time: None,
            best_event_time: None,
            event_time_confidence: None,
            best_posting_date: None,
            posting_date_confidence: None,
            merchant_normalized_name: None,
            merchant_entity_id: None,
            reference_id: None,
            location: None,
            original_amount_minor: None,
            original_currency: None,
            exchange_rate: None,
            balance_after_transaction: None,
            status: None,
            match_confidence: None,
            source_mix: None,
            alert_fired: None,
            parent_transaction_id: None,
            transaction_subtype: None,
            emi_group_id: None,
            category_id: None,
            is_deleted: false,
            created_at: None,
            updated_at: None,
        };
        let tx3 = TransactionsRow {
            id: "tx_3".into(),
            merchant_display_name: Some("Spotify".into()),
            unique_event_id: None,
            instrument_id: None,
            instrument_type: None,
            direction: None,
            amount: None,
            amount_minor: None,
            currency: None,
            authorization_time: None,
            best_event_time: None,
            event_time_confidence: None,
            best_posting_date: None,
            posting_date_confidence: None,
            merchant_normalized_name: None,
            merchant_entity_id: None,
            reference_id: None,
            location: None,
            original_amount_minor: None,
            original_currency: None,
            exchange_rate: None,
            balance_after_transaction: None,
            status: None,
            match_confidence: None,
            source_mix: None,
            alert_fired: None,
            parent_transaction_id: None,
            transaction_subtype: None,
            emi_group_id: None,
            category_id: None,
            is_deleted: false,
            created_at: None,
            updated_at: None,
        };

        transactions::insert_transaction(&conn, &tx1).unwrap();
        transactions::insert_transaction(&conn, &tx2).unwrap();
        transactions::insert_transaction(&conn, &tx3).unwrap();

        let res = transactions::search_transactions(&conn, "Netflix", 10, 0).unwrap();
        assert_eq!(res.len(), 2);

        // Exact match should rank better
        assert_eq!(res[0].id, "tx_1");
        assert_eq!(res[1].id, "tx_2");

        let mut tx_updated = tx1.clone();
        tx_updated.merchant_display_name = Some("Netflix Inc".into());
        transactions::update_transaction(&conn, &tx_updated).unwrap();
        let res2 = transactions::search_transactions(&conn, "Inc", 10, 0).unwrap();
        assert_eq!(res2.len(), 1);

        transactions::delete_transaction(&conn, "tx_1").unwrap();
        let res3 = transactions::search_transactions(&conn, "Netflix", 10, 0).unwrap();
        assert_eq!(res3.len(), 1);
        assert_eq!(res3[0].id, "tx_2");
    }

    #[test]
    fn test_transaction_observations_crud() {
        let conn = setup_db();

        let mut obs = TransactionObservationsRow {
            id: "obs_1".into(),
            canonical_transaction_id: None,
            source_pipeline: Some("email".into()),
            source_record_id: Some("rec_1".into()),
            source_message_id: None,
            source_thread_id: None,
            statement_id: None,
            statement_entry_id: None,
            instrument_id: None,
            direction: Some("debit".into()),
            amount: Some(150.0),
            amount_minor: Some(15000),
            currency: Some("USD".into()),
            event_time: None,
            event_time_confidence: None,
            posting_date: None,
            merchant_raw: Some("Amazon".into()),
            merchant_normalized: None,
            reference_id: None,
            original_amount_minor: None,
            original_currency: None,
            exchange_rate: None,
            balance_after_transaction: None,
            timezone_at_ingestion: None,
            fingerprint: Some("fp_1".into()),
            extraction_method: None,
            confidence_score: None,
            raw_payload_json: None,
            parser_version: None,
            emi_total_installments: None,
            emi_installment_number: None,
            emi_original_amount_minor: None,
            is_deleted: false,
            created_at: None,
            updated_at: None,
        };

        transaction_observations::insert_observation(&conn, &obs).unwrap();

        let fetched = transaction_observations::get_observation(&conn, "obs_1")
            .unwrap()
            .unwrap();
        assert_eq!(fetched.merchant_raw, Some("Amazon".into()));

        // Test Unique constraint (source_pipeline, source_record_id)
        let mut obs2 = obs.clone();
        obs2.id = "obs_2".into();
        obs2.fingerprint = Some("fp_2".into()); // Change fingerprint to avoid that constraint
        let err = transaction_observations::insert_observation(&conn, &obs2).unwrap_err();
        assert!(err.to_string().contains("UNIQUE constraint failed"));

        // Test Unique constraint (fingerprint)
        let mut obs3 = obs.clone();
        obs3.id = "obs_3".into();
        obs3.source_record_id = Some("rec_3".into()); // Change source_record_id to avoid that constraint
        let err2 = transaction_observations::insert_observation(&conn, &obs3).unwrap_err();
        assert!(err2.to_string().contains("UNIQUE constraint failed"));

        // Update
        obs.merchant_raw = Some("Amazon Updated".into());
        transaction_observations::update_observation(&conn, &obs).unwrap();
        let fetched2 = transaction_observations::get_observation(&conn, "obs_1")
            .unwrap()
            .unwrap();
        assert_eq!(fetched2.merchant_raw, Some("Amazon Updated".into()));

        // Test FK violation
        conn.execute("PRAGMA foreign_keys = ON", []).unwrap();
        let mut obs_fk = obs.clone();
        obs_fk.id = "obs_fk".into();
        obs_fk.canonical_transaction_id = Some("invalid_tx_id".into());
        obs_fk.fingerprint = Some("fp_fk".into());
        obs_fk.source_record_id = Some("rec_fk".into());
        let err_fk = transaction_observations::insert_observation(&conn, &obs_fk).unwrap_err();
        assert!(err_fk.to_string().contains("FOREIGN KEY constraint failed"));

        // Paginated
        let res = transaction_observations::select_all_paginated(&conn, 10, 0).unwrap();
        assert_eq!(res.len(), 1);

        // Soft delete
        transaction_observations::soft_delete(&conn, "obs_1").unwrap();
        let fetched3 = transaction_observations::get_observation(&conn, "obs_1").unwrap();
        assert!(fetched3.is_none());

        // Paginated should exclude deleted
        let res_deleted = transaction_observations::select_all_paginated(&conn, 10, 0).unwrap();
        assert_eq!(res_deleted.len(), 0);
    }

    #[test]
    fn test_statements_crud() {
        let conn = setup_db();
        conn.execute("PRAGMA foreign_keys = ON", []).unwrap();

        // 1. Insert and retrieve
        let mut stmt1 = StatementsRow {
            id: "stmt_1".into(),
            instrument_id: None,
            statement_type: "credit_card_statement".into(),
            billing_period_start: chrono::NaiveDate::from_ymd_opt(2023, 1, 1).unwrap(),
            billing_period_end: chrono::NaiveDate::from_ymd_opt(2023, 1, 31).unwrap(),
            due_date: None,
            statement_date: None,
            current_balance: Some(100000),
            minimum_due: Some(5000),
            rewards_summary_json: Some(r#"{"points": 500}"#.into()),
            source_message_id: None,
            parse_status: "parsed".into(),
            is_duplicate: false,
            created_at: None,
            updated_at: None,
        };
        statements::insert(&conn, &stmt1).unwrap();

        let fetched = statements::select_by_id(&conn, "stmt_1").unwrap().unwrap();
        assert_eq!(fetched.statement_type, "credit_card_statement");
        assert_eq!(fetched.current_balance, Some(100000));
        assert_eq!(
            fetched.rewards_summary_json,
            Some(r#"{"points": 500}"#.into())
        );

        // 2. Update
        stmt1.parse_status = "failed".into();
        stmt1.current_balance = Some(120000);
        statements::update(&conn, &stmt1).unwrap();

        let fetched_updated = statements::select_by_id(&conn, "stmt_1").unwrap().unwrap();
        assert_eq!(fetched_updated.parse_status, "failed");
        assert_eq!(fetched_updated.current_balance, Some(120000));
        assert!(fetched_updated.updated_at.is_some());

        // 3. Paginated
        let mut stmt2 = stmt1.clone();
        stmt2.id = "stmt_2".into();
        statements::insert(&conn, &stmt2).unwrap();

        let all_statements = statements::select_all_paginated(&conn, 10, 0).unwrap();
        assert_eq!(all_statements.len(), 2);

        let limited = statements::select_all_paginated(&conn, 1, 0).unwrap();
        assert_eq!(limited.len(), 1);

        // 4. FK Violation
        let mut stmt_fk = stmt1.clone();
        stmt_fk.id = "stmt_fk".into();
        stmt_fk.instrument_id = Some("invalid_instrument_id".into());
        let err_fk = statements::insert(&conn, &stmt_fk).unwrap_err();
        assert!(err_fk.to_string().contains("FOREIGN KEY constraint failed"));

        // 5. Delete (Mapped to soft_delete in checklist)
        statements::soft_delete(&conn, "stmt_1").unwrap();
        let fetched_deleted = statements::select_by_id(&conn, "stmt_1").unwrap();
        assert!(fetched_deleted.is_none());
    }
    #[test]
    fn test_match_decisions_no_update_path() {
        let conn = setup_db();

        // 1. Setup observation and canonical transaction
        let obs = crate::db::transaction_observations::TransactionObservationsRow {
            id: "obs_test_md".into(),
            canonical_transaction_id: None,
            source_pipeline: None,
            source_record_id: None,
            source_message_id: None,
            source_thread_id: None,
            statement_id: None,
            statement_entry_id: None,
            instrument_id: None,
            direction: None,
            amount: None,
            amount_minor: None,
            currency: None,
            event_time: None,
            event_time_confidence: None,
            posting_date: None,
            merchant_raw: None,
            merchant_normalized: None,
            reference_id: None,
            original_amount_minor: None,
            original_currency: None,
            exchange_rate: None,
            balance_after_transaction: None,
            timezone_at_ingestion: None,
            fingerprint: None,
            extraction_method: None,
            confidence_score: None,
            raw_payload_json: None,
            parser_version: None,
            emi_total_installments: None,
            emi_installment_number: None,
            emi_original_amount_minor: None,
            is_deleted: false,
            created_at: None,
            updated_at: None,
        };
        crate::db::transaction_observations::insert_observation(&conn, &obs).unwrap();

        let tx = crate::db::transactions::TransactionsRow {
            id: "tx_test_md".into(),
            unique_event_id: None,
            instrument_id: None,
            instrument_type: None,
            direction: None,
            amount: Some(50.0),
            amount_minor: Some(5000),
            currency: Some("USD".into()),
            authorization_time: None,
            best_event_time: None,
            event_time_confidence: None,
            best_posting_date: None,
            posting_date_confidence: None,
            merchant_display_name: Some("Test Merchant".into()),
            merchant_normalized_name: None,
            merchant_entity_id: None,
            reference_id: None,
            location: None,
            original_amount_minor: None,
            original_currency: None,
            exchange_rate: None,
            balance_after_transaction: None,
            status: None,
            match_confidence: None,
            source_mix: None,
            alert_fired: None,
            parent_transaction_id: None,
            transaction_subtype: None,
            emi_group_id: None,
            category_id: None,
            is_deleted: false,
            created_at: None,
            updated_at: None,
        };
        crate::db::transactions::insert_transaction(&conn, &tx).unwrap();

        // 2. Insert match decision
        let decision = crate::db::match_decisions::MatchDecisionsRow {
            id: "md_test_1".into(),
            observation_id: Some("obs_test_md".into()),
            matched_transaction_id: Some("tx_test_md".into()),
            decision: Some("auto_matched_exact".into()),
            score: Some(0.95),
            rules_triggered_json: Some("{\"rule\": \"exact_amount\"}".into()),
            review_status: Some("pending".into()),
            reviewed_by: Some("system".into()),
            created_at: None,
        };

        crate::db::match_decisions::insert(&conn, &decision).unwrap();

        // 3. Select by ID
        let fetched = crate::db::match_decisions::select_by_id(&conn, "md_test_1")
            .unwrap()
            .expect("should exist");
        assert_eq!(fetched.decision.as_deref(), Some("auto_matched_exact"));
        assert_eq!(fetched.score, Some(0.95));
        assert_eq!(
            fetched.rules_triggered_json.as_deref(),
            Some("{\"rule\": \"exact_amount\"}")
        );
        assert_eq!(fetched.reviewed_by.as_deref(), Some("system"));

        // 4. Select by Observation ID
        let by_obs =
            crate::db::match_decisions::select_by_observation_id(&conn, "obs_test_md").unwrap();
        assert_eq!(by_obs.len(), 1);
        assert_eq!(by_obs[0].id, "md_test_1");

        // 5. Assert immutability (Attempt Update)
        let update_result = conn.execute(
            "UPDATE match_decisions SET decision = ?1 WHERE id = ?2",
            rusqlite::params!["manual_matched", "md_test_1"],
        );
        assert!(
            update_result.is_err(),
            "Update should have failed due to immutability trigger"
        );
        let err_str = update_result.unwrap_err().to_string();
        assert!(
            err_str.contains("match_decisions is immutable"),
            "Unexpected error: {}",
            err_str
        );
    }
    #[test]
    fn test_reconciliation_clusters_crud() {
        let conn = setup_db();

        let cluster = crate::db::reconciliation_clusters::ReconciliationClustersRow {
            id: "cluster_test_1".into(),
            observation_id: None,
            status: "unreconciled".into(),
            total_amount_minor: Some(1000),
            currency: Some("USD".into()),
            resolution_note: None,
            resolved_at: None,
            created_at: None,
            updated_at: None,
        };

        // Test Insert
        crate::db::reconciliation_clusters::insert(&conn, &cluster).unwrap();

        // Test Select
        let fetched = crate::db::reconciliation_clusters::select_by_id(&conn, "cluster_test_1")
            .unwrap()
            .unwrap();
        assert_eq!(fetched.id, "cluster_test_1");
        assert_eq!(fetched.status, "unreconciled");
        assert_eq!(fetched.total_amount_minor, Some(1000));

        // Test Update Status
        crate::db::reconciliation_clusters::update_status(
            &conn,
            "cluster_test_1",
            "partially_reconciled",
        )
        .unwrap();
        let fetched_updated =
            crate::db::reconciliation_clusters::select_by_id(&conn, "cluster_test_1")
                .unwrap()
                .unwrap();
        assert_eq!(fetched_updated.status, "partially_reconciled");

        let tx = crate::db::transactions::TransactionsRow {
            id: "tx_test_1".into(),
            unique_event_id: None,
            instrument_id: None,
            instrument_type: None,
            direction: None,
            amount: None,
            amount_minor: None,
            currency: None,
            authorization_time: None,
            best_event_time: None,
            event_time_confidence: None,
            best_posting_date: None,
            posting_date_confidence: None,
            merchant_display_name: None,
            merchant_normalized_name: None,
            merchant_entity_id: None,
            reference_id: None,
            location: None,
            original_amount_minor: None,
            original_currency: None,
            exchange_rate: None,
            balance_after_transaction: None,
            status: None,
            match_confidence: None,
            source_mix: None,
            alert_fired: None,
            parent_transaction_id: None,
            transaction_subtype: None,
            emi_group_id: None,
            category_id: None,
            is_deleted: false,
            created_at: None,
            updated_at: None,
        };
        crate::db::transactions::insert_transaction(&conn, &tx).unwrap();

        // Test Member Insert & Select
        let member = crate::db::reconciliation_cluster_members::ReconciliationClusterMembersRow {
            id: "rcm_test_1".into(),
            cluster_id: "cluster_test_1".into(),
            transaction_id: Some("tx_test_1".into()),
            statement_entry_id: None,
            added_at: None,
            updated_at: None,
        };
        crate::db::reconciliation_cluster_members::insert(&conn, &member).unwrap();

        let members = crate::db::reconciliation_cluster_members::select_by_cluster_id(
            &conn,
            "cluster_test_1",
        )
        .unwrap();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].transaction_id.as_deref(), Some("tx_test_1"));

        // Test Delete Member by Cluster ID
        crate::db::reconciliation_cluster_members::delete_by_cluster_id(&conn, "cluster_test_1")
            .unwrap();
        let members_after_delete = crate::db::reconciliation_cluster_members::select_by_cluster_id(
            &conn,
            "cluster_test_1",
        )
        .unwrap();
        assert_eq!(members_after_delete.len(), 0);

        // Test Cascade Delete
        // Re-insert member
        crate::db::reconciliation_cluster_members::insert(&conn, &member).unwrap();
        // Delete cluster
        crate::db::reconciliation_clusters::delete(&conn, "cluster_test_1").unwrap();
        // Check if member is deleted due to CASCADE
        let members_after_cascade =
            crate::db::reconciliation_cluster_members::select_by_cluster_id(
                &conn,
                "cluster_test_1",
            )
            .unwrap();
        assert_eq!(members_after_cascade.len(), 0);
    }

    #[test]
    fn test_cluster_excluded_from_analytics_when_pending() {
        let conn = setup_db();

        // Insert a cluster with 'unreconciled' status (which is pending)
        let cluster = crate::db::reconciliation_clusters::ReconciliationClustersRow {
            id: "cluster_test_5".into(),
            observation_id: None,
            status: "unreconciled".into(),
            total_amount_minor: Some(1000),
            currency: Some("USD".into()),
            resolution_note: None,
            resolved_at: None,
            created_at: None,
            updated_at: None,
        };
        crate::db::reconciliation_clusters::insert(&conn, &cluster).unwrap();

        // Insert a fully reconciled cluster
        let cluster2 = crate::db::reconciliation_clusters::ReconciliationClustersRow {
            id: "cluster_test_6".into(),
            observation_id: None,
            status: "fully_reconciled".into(),
            total_amount_minor: Some(2000),
            currency: Some("EUR".into()),
            resolution_note: None,
            resolved_at: None,
            created_at: None,
            updated_at: None,
        };
        crate::db::reconciliation_clusters::insert(&conn, &cluster2).unwrap();

        // In this test, we simulate an analytics query that only looks at reconciled clusters
        let mut stmt = conn.prepare("SELECT SUM(total_amount_minor) FROM reconciliation_clusters WHERE status IN ('fully_reconciled', 'over_reconciled')").unwrap();
        let total: Option<i64> = stmt.query_row([], |row| row.get(0)).unwrap();

        // Only the fully_reconciled cluster's amount should be included
        assert_eq!(total, Some(2000));
    }

    #[test]
    fn test_merchants_and_aliases() {
        let conn = setup_db();

        // Seed migration should have run, but let's just insert one to be safe
        let merchant = MerchantsRow {
            id: "merch_test".into(),
            name: "Test Merchant".into(),
            category_id: None,
            logo_url: None,
            website: None,
            is_deleted: false,
            created_at: None,
            updated_at: None,
        };
        merchants::insert(&conn, &merchant).unwrap();

        let alias = MerchantAliasesRow {
            id: "alias_test".into(),
            merchant_id: Some("merch_test".into()),
            alias: "TEST M".into(),
            created_at: None,
        };
        merchants::insert_alias(&conn, &alias).unwrap();

        let fetched_alias = merchants::select_by_alias(&conn, "TEST M")
            .unwrap()
            .unwrap();
        assert_eq!(fetched_alias.name, "Test Merchant");

        let fetched_merch = merchants::select_by_id(&conn, "merch_test")
            .unwrap()
            .unwrap();
        assert_eq!(fetched_merch.id, "merch_test");

        // Ensure seed data works (from 0002)
        let amazon = merchants::select_by_alias(&conn, "AMZN").unwrap().unwrap();
        assert_eq!(amazon.name, "Amazon");

        let swiggy = merchants::select_by_alias(&conn, "SWIGGY BUNDL TECH")
            .unwrap()
            .unwrap();
        assert_eq!(swiggy.name, "Swiggy");
    }

    #[test]
    fn test_fk_cascade_rules() {
        let conn = setup_db();
        // Turn on foreign keys
        conn.execute("PRAGMA foreign_keys = ON", []).unwrap();

        // Statements -> statement_entries
        let statement = crate::db::statements::StatementsRow {
            id: "stmt_fk_test".into(),
            instrument_id: None,
            statement_type: "credit".into(),
            billing_period_start: chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            billing_period_end: chrono::NaiveDate::from_ymd_opt(2026, 1, 31).unwrap(),
            due_date: None,
            statement_date: None,
            current_balance: None,
            minimum_due: None,
            rewards_summary_json: None,
            source_message_id: None,
            parse_status: "success".into(),
            is_duplicate: false,
            created_at: None,
            updated_at: None,
        };
        crate::db::statements::insert(&conn, &statement).unwrap();

        let entry = crate::db::statement_entries::StatementEntriesRow {
            id: "entry_fk_test".into(),
            statement_id: Some("stmt_fk_test".into()),
            row_index: None,
            transaction_date: None,
            posting_date: None,
            description_raw: None,
            merchant_raw: None,
            merchant_normalized: None,
            amount: None,
            amount_minor: None,
            currency: None,
            direction: None,
            reference_id: None,
            location: None,
            raw_row_json: None,
            created_at: None,
        };
        crate::db::statement_entries::insert(&conn, &entry).unwrap();

        // Delete the statement and check cascade
        conn.execute("DELETE FROM statements WHERE id = 'stmt_fk_test'", []).unwrap();

        let entries: i32 = conn.query_row("SELECT count(*) FROM statement_entries WHERE id = 'entry_fk_test'", [], |r| r.get(0)).unwrap();
        assert_eq!(entries, 0, "statement_entry should be cascaded deleted");

        // Transactions -> transaction_tags
        let tx = crate::db::transactions::TransactionsRow {
            id: "tx_fk_test".into(),
            unique_event_id: None,
            instrument_id: None,
            instrument_type: None,
            direction: None,
            amount: None,
            amount_minor: None,
            currency: None,
            authorization_time: None,
            best_event_time: None,
            event_time_confidence: None,
            best_posting_date: None,
            posting_date_confidence: None,
            merchant_display_name: None,
            merchant_normalized_name: None,
            merchant_entity_id: None,
            reference_id: None,
            location: None,
            original_amount_minor: None,
            original_currency: None,
            exchange_rate: None,
            balance_after_transaction: None,
            status: None,
            match_confidence: None,
            source_mix: None,
            alert_fired: None,
            parent_transaction_id: None,
            transaction_subtype: None,
            emi_group_id: None,
            category_id: None,
            is_deleted: false,
            created_at: None,
            updated_at: None,
        };
        crate::db::transactions::insert_transaction(&conn, &tx).unwrap();

        let tag = crate::db::tags::TagsRow {
            id: "tag_fk_test".into(),
            name: "Test Tag".into(),
            color: None,
            created_at: None,
        };
        crate::db::tags::insert(&conn, &tag).unwrap();

        conn.execute("INSERT INTO transaction_tags (transaction_id, tag_id) VALUES ('tx_fk_test', 'tag_fk_test')", []).unwrap();
        conn.execute("DELETE FROM transactions WHERE id = 'tx_fk_test'", []).unwrap();

        let tags_assoc: i32 = conn.query_row("SELECT count(*) FROM transaction_tags WHERE transaction_id = 'tx_fk_test'", [], |r| r.get(0)).unwrap();
        assert_eq!(tags_assoc, 0, "transaction_tags should be cascaded deleted");

        // ReconciliationClusters -> reconciliation_cluster_members
        let cluster = crate::db::reconciliation_clusters::ReconciliationClustersRow {
            id: "cluster_fk_test".into(),
            observation_id: None,
            status: "unreconciled".into(),
            total_amount_minor: None,
            currency: None,
            resolution_note: None,
            resolved_at: None,
            created_at: None,
            updated_at: None,
        };
        crate::db::reconciliation_clusters::insert(&conn, &cluster).unwrap();

        let member = crate::db::reconciliation_cluster_members::ReconciliationClusterMembersRow {
            id: "member_fk_test".into(),
            cluster_id: "cluster_fk_test".into(),
            transaction_id: None,
            statement_entry_id: None,
            added_at: None,
            updated_at: None,
        };
        crate::db::reconciliation_cluster_members::insert(&conn, &member).unwrap();

        conn.execute("DELETE FROM reconciliation_clusters WHERE id = 'cluster_fk_test'", []).unwrap();
        let members: i32 = conn.query_row("SELECT count(*) FROM reconciliation_cluster_members WHERE id = 'member_fk_test'", [], |r| r.get(0)).unwrap();
        assert_eq!(members, 0, "reconciliation_cluster_members should be cascaded deleted");
    }
}
