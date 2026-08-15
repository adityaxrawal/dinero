    use super::*;

    #[test]
    fn test_bank_template_integrity() {
        let templates = bank_templates();
        assert!(
            !templates.is_empty(),
            "no bank templates compiled -- build.rs glob produced an empty set"
        );

        for (bank, patterns) in templates {
            assert!(
                !patterns.is_empty(),
                "{bank}: template file has zero patterns"
            );
            for p in patterns {
                let groups = p.regex.captures_len();
                let named = [
                    ("amount_group", Some(p.amount_group)),
                    ("merchant_group", p.merchant_group),
                    ("date_group", p.date_group),
                    ("balance_group", p.balance_group),
                    ("reference_group", p.reference_group),
                    ("last4_group", p.last4_group),
                    ("upi_vpa_group", p.upi_vpa_group),
                    ("cadence_group", p.cadence_group),
                ];
                for (field, group) in named {
                    if let Some(g) = group {
                        assert!(
                            g > 0 && g < groups,
                            "{bank}/{}: {field}={g} but the regex has {} capture groups \
                             (valid indices 1..{}) -- this field would silently never populate",
                            p.name,
                            groups - 1,
                            groups - 1
                        );
                    }
                }
                assert!(
                    p.direction == "debit" || p.direction == "credit",
                    "{bank}/{}: direction {:?} must be \"debit\" or \"credit\"",
                    p.name,
                    p.direction
                );
                if let Some(t) = p.txn_type.as_deref() {
                    assert!(
                        TEMPLATE_TXN_TYPES.contains(&t),
                        "{bank}/{}: txn_type {t:?} is not one of {TEMPLATE_TXN_TYPES:?}",
                        p.name
                    );
                }
                assert!(
                    p.currency.len() == 3 && p.currency.chars().all(|c| c.is_ascii_uppercase()),
                    "{bank}/{}: currency {:?} must be a 3-letter uppercase ISO code",
                    p.name,
                    p.currency
                );
                if p.txn_type.as_deref() == Some("mandate") {
                    assert!(
                        p.merchant_group.is_some(),
                        "{bank}/{}: a mandate pattern needs a merchant_group",
                        p.name
                    );
                }
                if p.txn_type.as_deref() != Some("account_balance")
                    && p.txn_type.as_deref() != Some("mandate")
                {
                    assert!(
                        p.merchant_group.is_some() || p.balance_group.is_some(),
                        "{bank}/{}: no merchant_group and no balance_group -- \
                         `ExtractionResult::is_valid()` can never pass for this pattern",
                        p.name
                    );
                }
            }
        }
    }

    #[tokio::test]
    async fn bank_template_confidence_outranks_generic_regex() {
        let pool = dummy_pool();
        let got = BankTemplateLayer
            .extract(
                &pool,
                "Jupiter",
                "Hey, Aditya Your UPI payment was successful You paid ₹543 Paid to \
                 HONGKONG NOODLES Vyapar.169687998887@hdfcbank Date Jan 01, 2026 From \
                 Aditya 8127696200@jupiteraxis Transaction ID 1321767280821724605",
            )
            .await
            .expect("template must still match");

        let confidence = got
            .confidence_score
            .expect("Doc 30 TASK-TXN-004 gives Layers 1/2 a band; NULL is not it");
        assert!(
            confidence > LAYER3_MAX_CONFIDENCE,
            "a template match ({confidence}) must outrank the best possible \
             generic-regex result ({LAYER3_MAX_CONFIDENCE}), or precedence \
             cannot prefer it"
        );
        assert!(confidence >= 0.9, "Doc 30 says Layer 1/2 is typically 0.9+");
    }

    #[tokio::test]
    async fn test_tier1_templates_extract_real_bodies() {
        struct Case {
            bank: &'static str,
            body: &'static str,
            amount_minor: i64,
            direction: &'static str,
            merchant: Option<&'static str>,
            last4: Option<&'static str>,
            date: Option<(i32, u32, u32)>,
            balance_minor: Option<i64>,
        }

        let cases = [
            Case {
                bank: "HDFC Bank",
                body: "Dear Customer, Rs.200.00 has been debited from account 4691 to VPA \
                       shreesomnathtrustvas.76061863@hdfcbank SHREE SOMNATH TRUST VAS on \
                       23-12-25. Your UPI transaction reference number is 533264925852.",
                amount_minor: 20000,
                direction: "debit",
                merchant: Some("SHREE SOMNATH TRUST VAS"),
                last4: Some("4691"),
                date: Some((2025, 12, 23)),
                balance_minor: None,
            },
            Case {
                bank: "HDFC Bank",
                body: "Dear Card Member, Thank you for using your HDFC Bank Credit Card ending \
                       0364 for Rs 706.00 at Payu*Swiggy Food on 07-08-2025 19:25:29. \
                       Authorization code:- 002587",
                amount_minor: 70600,
                direction: "debit",
                merchant: Some("Payu*Swiggy Food"),
                last4: Some("0364"),
                date: Some((2025, 8, 7)),
                balance_minor: None,
            },
            Case {
                bank: "HDFC Bank",
                body: "Dear Customer, Here is the update on your account balance: As of \
                       yesterday, 04-SEP-25 available balance is INR 10050.00 in your A/c XX4691",
                amount_minor: 1005000,
                direction: "credit",
                merchant: None,
                last4: Some("4691"),
                date: Some((2025, 9, 4)),
                balance_minor: Some(1005000),
            },
            Case {
                bank: "SBI Card",
                body: "SBI Card TRANSACTION ALERT! Dear Cardholder, This is to inform you that, \
                       Rs.480.20 spent on your SBI Credit Card ending 7603 at \
                       INNOVATIVERETAILCONCEPT on 10/01/26.",
                amount_minor: 48020,
                direction: "debit",
                merchant: Some("INNOVATIVERETAILCONCEPT"),
                last4: Some("7603"),
                date: Some((2026, 1, 10)),
                balance_minor: None,
            },
            Case {
                bank: "IDFC FIRST Bank",
                body: "Dear Cardmember, Delicious Purchase! INR 725.00 spent on your IDFC FIRST \
                       BANK Credit Card ending XX3620 at TRUFFLES HOSPITALITY PVT on 05 AUG \
                       2025. Available Limit: INR 38666.18 .",
                amount_minor: 72500,
                direction: "debit",
                merchant: Some("TRUFFLES HOSPITALITY PVT"),
                last4: Some("3620"),
                date: Some((2025, 8, 5)),
                balance_minor: Some(3866618),
            },
            Case {
                bank: "IDFC FIRST Bank",
                body: "Dear Customer, Payment of Rs. 6,283.37 was received on your FIRST \
                       Millennia Credit Card ending with XX3620 on 29 Nov 2025.",
                amount_minor: 628337,
                direction: "credit",
                merchant: Some("FIRST Millennia"),
                last4: Some("3620"),
                date: Some((2025, 11, 29)),
                balance_minor: None,
            },
            Case {
                bank: "Axis Bank",
                body: "17-08-2025 Dear Aditya Rawal, Thank you for using your credit card no. \
                       XX3825 for INR 379 at AIRTEL PAYM on 17-08-2025 00:41:37 IST.",
                amount_minor: 37900,
                direction: "debit",
                merchant: Some("AIRTEL PAYM"),
                last4: Some("3825"),
                date: Some((2025, 8, 17)),
                balance_minor: None,
            },
            Case {
                bank: "Yes Bank",
                body: "Dear Cardmember, INR 2441.98 has been spent on your YES BANK Credit Card \
                       ending with 2982 at UPI_RELIANCE BP MOBILI on 26-10-2025 at 01:47:41 pm. \
                       Avl Bal INR 95138.98.",
                amount_minor: 244198,
                direction: "debit",
                merchant: Some("UPI_RELIANCE BP MOBILI"),
                last4: Some("2982"),
                date: Some((2025, 10, 26)),
                balance_minor: Some(9513898),
            },
            Case {
                bank: "Jupiter",
                body: "Hey, Aditya Your UPI payment was successful You paid ₹543 Paid to \
                       HONGKONG NOODLES Vyapar.169687998887@hdfcbank Date Jan 01, 2026 From \
                       Aditya 8127696200@jupiteraxis Transaction ID 1321767280821724605",
                amount_minor: 54300,
                direction: "debit",
                merchant: Some("HONGKONG NOODLES"),
                last4: None,
                date: Some((2026, 1, 1)),
                balance_minor: None,
            },
        ];

        let pool = dummy_pool();
        for c in cases {
            let got = BankTemplateLayer
                .extract(&pool, c.bank, c.body)
                .await
                .unwrap_or_else(|| panic!("{}: no template matched a real body", c.bank));

            assert_eq!(got.amount_minor, Some(c.amount_minor), "{} amount", c.bank);
            assert_eq!(
                got.direction.as_deref(),
                Some(c.direction),
                "{} direction",
                c.bank
            );
            assert_eq!(
                got.merchant_raw.as_deref(),
                c.merchant,
                "{} merchant",
                c.bank
            );
            assert_eq!(
                got.masked_identifier.as_deref(),
                c.last4,
                "{} last4",
                c.bank
            );
            assert_eq!(
                got.balance_after, c.balance_minor,
                "{} balance_after",
                c.bank
            );
            if let Some((y, m, d)) = c.date {
                assert_eq!(
                    got.event_time,
                    Some(ymd_ts(y, m, d)),
                    "{} event_time",
                    c.bank
                );
            }
            assert!(
                got.is_valid(),
                "{}: template matched but the result fails is_valid(), so the ladder \
                 would discard it and fall through to Layer 3",
                c.bank
            );
        }
    }

    #[tokio::test]
    async fn test_mandate_pattern_routes_to_mandate_extractor_only() {
        let body = "Dear Cardholder, Thank you for registering for a recurring e-Mandate at \
                    merchant platform using your SBI Credit Card. Your e-Mandate set at merchant \
                    with SBI Credit Card ending 7603 has been registered. Merchant: ScribdInc \
                    Description: PremiumMonthlyMembership e-Mandate Limit Amount (INR): 1000.00 \
                    Frequency: monthly Start date: 21/04/2026 SiHub ID: YPCojLhIn2";

        let m = crate::extraction::mandate_extractor::bank_mandate_template("SBI Card", body)
            .expect("SBI Card mandate template must match a real registration body");
        assert_eq!(m.merchant.as_deref(), Some("ScribdInc"));
        assert_eq!(m.cadence.as_deref(), Some("monthly"));
        assert_eq!(m.max_limit_amount, Some(100_000));
        assert_eq!(m.external_mandate_id.as_deref(), Some("YPCojLhIn2"));
        assert_eq!(m.masked_identifier.as_deref(), Some("7603"));
        assert_eq!(m.instrument_type.as_deref(), Some("credit_card"));

        let as_txn = BankTemplateLayer
            .extract(&dummy_pool(), "SBI Card", body)
            .await;
        assert!(
            as_txn.is_none_or(|r| r.amount_minor != Some(100_000)),
            "a mandate limit must not be booked as a transaction amount"
        );
    }

    #[test]
    fn test_txn_types_map_to_valid_instrument_enum() {
        const SCHEMA_INSTRUMENT_TYPES: &[&str] = &[
            "credit_card",
            "debit_card",
            "bank_account",
            "UPI",
            "NEFT",
            "RTGS",
            "SWIFT",
            "upi_vpa",
            "wallet",
            "POS",
            "ATM",
            "cheque",
        ];
        for t in TEMPLATE_TXN_TYPES {
            if let Some(mapped) = instrument_type_for_txn_type(t) {
                assert!(
                    SCHEMA_INSTRUMENT_TYPES.contains(&mapped.as_str()),
                    "txn_type {t:?} maps to instrument_type {mapped:?}, which the \
                     instruments.type CHECK constraint would reject"
                );
            }
        }
        assert_eq!(
            instrument_type_for_txn_type("mandate"),
            None,
            "a mandate is an authorisation, not an instrument"
        );
    }

    #[test]
    fn test_every_registry_bank_has_a_template() {
        let registry: crate::ingestion::verified_senders::VerifiedSenderRegistry =
            serde_json::from_str(include_str!("../../ingestion/verified_senders_registry.json"))
                .expect("registry must parse");

        let templates = bank_templates();
        for sender in &registry.senders {
            if sender.classification == "noise" {
                continue;
            }
            assert!(
                templates.contains_key(&sender.bank_name),
                "registry bank {:?} (domain {}) has no bank_templates/*.json -- \
                 it would silently skip Layer 2 entirely",
                sender.bank_name,
                sender.domain
            );
        }

        let registry_banks: std::collections::HashSet<&str> = registry
            .senders
            .iter()
            .map(|s| s.bank_name.as_str())
            .collect();
        for bank in templates.keys() {
            assert!(
                registry_banks.contains(bank.as_str()),
                "bank_templates has {bank:?} but no registry sender maps to it -- \
                 stale file, or the registry renamed the bank"
            );
        }
    }

    fn dummy_pool() -> Pool {
        let mgr = deadpool_sqlite::Manager::from_config(
            &deadpool_sqlite::Config {
                path: ":memory:".into(),
                pool: Some(deadpool_sqlite::PoolConfig::new(1)),
            },
            deadpool_sqlite::Runtime::Tokio1,
        );
        Pool::builder(mgr).build().unwrap()
    }

    #[tokio::test]
    async fn test_sbi_intro_clause_boilerplate_does_not_win_over_real_merchant() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "Dear Cardholder,\nThis is to inform you that, Rs.245.43 spent on your SBI Credit Card ending 7603 at DREAMPLUGTECHNOLOGI on 01/07/26. Trxn. not done by you? Report at https://sbicard.com/Dispute. If you have not authorized this transaction please contact the SBI Card Helpline.";
        let result = layer.extract(&pool, "SBI Card", body).await.unwrap();
        assert_eq!(result.merchant_raw, Some("DREAMPLUGTECHNOLOGI".to_string()));
    }

    #[tokio::test]
    async fn test_upi_p2p_transfer_falls_back_to_vpa_handle() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "Dear Customer,\n\nGreetings from HDFC Bank!\n\nRs.750.00 is debited from your account ending 4691 towards VPA 8127696200@jupiteraxis (ADITYA RAWAL) on 07-06-26.\n\nUPI transaction reference no.: 327479321586.\n\nIf you did not authorize this transaction, please report it immediately at:\na. When in India (Toll free): 1800 258 6161\nb. When abroad:  9122 61606160\nc. Or SMS 'BLOCK UPI' to 7308080808.";
        let result = layer.extract(&pool, "HDFC Bank", body).await.unwrap();
        assert_eq!(
            result.merchant_raw,
            Some("8127696200@jupiteraxis".to_string())
        );
    }

    async fn dummy_migrated_pool() -> Pool {
        let db_path = crate::db::test_helpers::fresh_temp_db_path();
        crate::db::migrations::run_migrations(&db_path, None)
            .await
            .unwrap();
        let mgr = deadpool_sqlite::Manager::from_config(
            &deadpool_sqlite::Config {
                path: db_path,
                pool: Some(deadpool_sqlite::PoolConfig::new(1)),
            },
            deadpool_sqlite::Runtime::Tokio1,
        );
        Pool::builder(mgr).build().unwrap()
    }

    #[tokio::test]
    async fn test_orchestrator_stops_at_first_valid_layer() {
        let pool = setup_db_with_rule("active".to_string()).await;
        let body = "Your amount is 1500 INR at Amazon debit time 1700000000";

        let mut layer6_timed_out = false;
        let result = run_extraction_ladder(
            &pool,
            "Chase",
            body,
            None,
            false,
            None,
            &mut layer6_timed_out,
            None,
            &mut crate::logging::EmailTrace::new("test"))
        .await
        .unwrap();

        assert!(result.is_some());
        assert_eq!(result.unwrap().extraction_method, "learned_fields");
    }

    #[tokio::test]
    async fn test_learned_merchant_rule_overrides_a_later_layers_merchant() {
        let body = "Rs 500.00 debited at RAZ*SWIGGY on 25-May-23 towards purchase";
        let pool = dummy_migrated_pool().await;

        let mut timed_out = false;
        let before = run_extraction_ladder(
            &pool,
            "HDFC Bank",
            body,
            None,
            false,
            None,
            &mut timed_out,
            None,
            &mut crate::logging::EmailTrace::new("test"))
        .await
        .unwrap()
        .expect("fixture must extract");
        let baseline_merchant = before.merchant_raw.clone();

        let conn = pool.get().await.unwrap();
        let body_owned = body.to_string();
        conn.interact(move |c| {
            let template_hash = compute_template_hash(&body_owned);
            let pattern =
                crate::extraction::rule_synthesis::synthesize_span_regex(&body_owned, "RAZ*SWIGGY")
                    .expect("must synthesize a pattern");
            seed_rule(
                c,
                "HDFC Bank",
                "merchant",
                &body_owned,
                serde_json::json!({ "regex": pattern, "capture_group": 1 }),
                "active",
            );
            let _ = template_hash;
        })
        .await
        .unwrap();
        drop(conn);

        let after = run_extraction_ladder(
            &pool,
            "HDFC Bank",
            body,
            None,
            false,
            None,
            &mut timed_out,
            None,
            &mut crate::logging::EmailTrace::new("test"))
        .await
        .unwrap()
        .expect("fixture must still extract");

        assert_eq!(
            after.merchant_raw.as_deref(),
            Some("RAZ*SWIGGY"),
            "the learned rule must decide the merchant (baseline was {baseline_merchant:?})"
        );
    }

    #[tokio::test]
    async fn test_learned_merchant_rule_does_not_leak_to_other_email_shapes() {
        let taught_body = "Rs 500.00 debited at RAZ*SWIGGY on 25-May-23 towards purchase";
        let other_body =
            "INR 250.00 spent using your card at BIG BAZAAR on 26-May-23 towards purchase";
        let pool = dummy_migrated_pool().await;

        let conn = pool.get().await.unwrap();
        let taught = taught_body.to_string();
        conn.interact(move |c| {
            let template_hash = compute_template_hash(&taught);
            let pattern =
                crate::extraction::rule_synthesis::synthesize_span_regex(&taught, "RAZ*SWIGGY")
                    .unwrap();
            seed_rule(
                c,
                "HDFC Bank",
                "merchant",
                &taught,
                serde_json::json!({ "regex": pattern, "capture_group": 1 }),
                "active",
            );
            let _ = template_hash;
        })
        .await
        .unwrap();
        drop(conn);

        let mut timed_out = false;
        let other = run_extraction_ladder(
            &pool,
            "HDFC Bank",
            other_body,
            None,
            false,
            None,
            &mut timed_out,
            None,
            &mut crate::logging::EmailTrace::new("test"))
        .await
        .unwrap()
        .expect("the unrelated email must still extract");

        assert_ne!(
            other.merchant_raw.as_deref(),
            Some("RAZ*SWIGGY"),
            "a rule taught on one email shape must not rewrite a different one"
        );
    }

    #[tokio::test]
    async fn test_ensemble_lite_amount_disagreement_downgrades_confidence() {
        let body = "Txn ID 999900 INR for your purchase. Rs 500.00 debited at Amazon on 25-May-23";
        let pool = dummy_migrated_pool().await;
        let conn = pool.get().await.unwrap();
        let body_owned = body.to_string();
        conn.interact(move |c| {
            let template_hash = compute_template_hash(&body_owned);
            for (field, regex) in [
                ("amount", r"Txn ID (\d+)"),
                ("merchant", "at ([A-Za-z]+)"),
                ("currency", "([A-Z]{3})"),
                ("direction", "(debited)"),
                ("event_time", r"on (\d{2}-[A-Za-z]{3}-\d{2})"),
            ] {
                seed_rule(
                    c,
                    "WrongRuleBank",
                    field,
                    &body_owned,
                    serde_json::json!({"regex": regex, "capture_group": 1}),
                    "active",
                );
            }
            let _ = template_hash;
        })
        .await
        .unwrap();

        let mut layer6_timed_out = false;
        let result = run_extraction_ladder(
            &pool,
            "WrongRuleBank",
            body,
            None,
            false,
            None,
            &mut layer6_timed_out,
            None,
            &mut crate::logging::EmailTrace::new("test"))
        .await
        .unwrap()
        .expect("the (wrong) learned rule is schema-valid and must still be returned");

        assert_eq!(result.extraction_method, "learned_fields");
        assert_eq!(
            result.amount_minor,
            Some(99_990_000),
            "the buggy rule's own (wrong) captured amount is still what's returned"
        );
        assert_eq!(
            result.confidence_score,
            Some(CROSS_CHECK_DISAGREEMENT_CONFIDENCE),
            "disagreement with the independent Rs 500.00 signal must downgrade confidence, \
             even though Layer 1 itself never set one before"
        );
    }

    #[tokio::test]
    async fn test_orchestrator_fails_if_all_layers_empty() {
        use tracing_subscriber::layer::SubscriberExt;
        struct NoopLayer;
        impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for NoopLayer {}
        let _guard =
            tracing::subscriber::set_default(tracing_subscriber::registry().with(NoopLayer));

        let pool = dummy_pool();
        let mut layer6_timed_out = false;
        let res = run_extraction_ladder(
            &pool,
            "Chase",
            "unparseable body",
            None,
            false,
            None,
            &mut layer6_timed_out,
            None,
            &mut crate::logging::EmailTrace::new("test"))
        .await
        .unwrap();
        assert!(res.is_none());
    }

    #[tokio::test]
    async fn test_llm_skipped_when_ineligible() {
        use tracing_subscriber::layer::SubscriberExt;

        struct MessageVisitor(String);
        impl tracing::field::Visit for MessageVisitor {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if field.name() == "message" {
                    self.0 = format!("{:?}", value);
                }
            }
        }
        struct CapturingLayer(std::sync::Arc<std::sync::Mutex<Vec<String>>>);
        impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for CapturingLayer {
            fn on_event(
                &self,
                event: &tracing::Event<'_>,
                _ctx: tracing_subscriber::layer::Context<'_, S>,
            ) {
                let mut visitor = MessageVisitor(String::new());
                event.record(&mut visitor);
                self.0.lock().unwrap().push(visitor.0);
            }
        }

        let logs = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let subscriber = tracing_subscriber::registry().with(CapturingLayer(logs.clone()));
        let _guard = tracing::subscriber::set_default(subscriber);

        let pool = dummy_pool();
        let mut layer6_timed_out = false;
        let res = run_extraction_ladder(
            &pool,
            "Chase",
            "unparseable body",
            None,
            false,
            None,
            &mut layer6_timed_out,
            None,
            &mut crate::logging::EmailTrace::new("test"))
        .await
        .unwrap();
        assert!(res.is_none());

        let captured = logs.lock().unwrap();
        assert!(
            captured.iter().any(|l| l.contains("Layer 6 (LLM) skipped")),
            "expected the RAM-ineligibility skip log line, got: {:?}",
            *captured
        );
        assert!(
            !captured.iter().any(|l| l.contains("No app_dir provided")),
            "Layer6LlmLayer::extract must never be reached when llm_eligible is false, got: {:?}",
            *captured
        );
    }

    #[test]
    fn test_compute_template_hash() {
        let b1 = "Hello 123 World 456";
        let b2 = "hello   789 world 000";
        assert_eq!(compute_template_hash(b1), compute_template_hash(b2));
        assert_eq!(
            compute_template_hash(b1),
            "89a6278bc760568ecab7942236a60ca7d96b7ebcf19b98302c4465d2d6485c0b",
            "template hash changed -- every persisted template_hash is now orphaned"
        );
    }

    fn seed_rule(
        conn: &rusqlite::Connection,
        bank: &str,
        field: &str,
        source_body: &str,
        payload: serde_json::Value,
        status: &str,
    ) {
        let now = chrono::Utc::now().naive_utc();
        crate::db::field_rules::upsert_variant(
            conn,
            &crate::db::field_rules::FieldRuleVariant {
                id: uuid::Uuid::new_v4().to_string(),
                bank_name: bank.to_string(),
                field_name: field.to_string(),
                source_type: "email".to_string(),
                template_hash: compute_template_hash(source_body),
                rule_payload_json: payload,
                status: status.to_string(),
                success_count: 5,
                failure_count: 0,
                confidence: 1.0,
                authored_by: "deterministic".to_string(),
                learned_from: "user_edit".to_string(),
                created_at: Some(now),
                updated_at: Some(now),
            },
            None,
        )
        .unwrap();
    }

    const LEARNED_RULE_BODY: &str = "Your amount is 1500 INR at Amazon debit time 1700000000";

    async fn setup_db_with_rule(status: String) -> Pool {
        let pool = dummy_migrated_pool().await;
        let conn = pool.get().await.unwrap();
        conn.interact(move |c| {
            for (field, regex) in [
                ("amount", "amount is ([0-9]+) INR"),
                ("merchant", "at ([A-Za-z]+)"),
                ("currency", "([A-Z]{3})"),
                ("direction", "(debit)"),
                ("event_time", "time ([0-9]+)"),
            ] {
                seed_rule(
                    c,
                    "Chase",
                    field,
                    LEARNED_RULE_BODY,
                    serde_json::json!({"regex": regex, "capture_group": 1}),
                    &status,
                );
            }
        })
        .await
        .unwrap();
        pool
    }

    #[tokio::test]
    async fn a_merchant_only_rule_overrides_the_winning_layer() {
        let pool = dummy_migrated_pool().await;
        let body = "Rs 500 spent at RAZ*SWIGGY LIMITE on 01/07/26 via card 1234";
        {
            let conn = pool.get().await.unwrap();
            let b = body.to_string();
            conn.interact(move |c| {
                seed_rule(
                    c,
                    "HDFC Bank",
                    "merchant",
                    &b,
                    serde_json::json!({"regex": r"at\s+(.{1,80}?)\s+on", "capture_group": 1}),
                    "active",
                )
            })
            .await
            .unwrap();
        }

        let mut result = ExtractionResult {
            merchant_raw: Some("WRONG".to_string()),
            amount_minor: Some(50000),
            currency: Some("INR".to_string()),
            direction: Some("debit".to_string()),
            event_time: Some(1_780_000_000),
            ..Default::default()
        };
        let fired = apply_learned_fields(&pool, "HDFC Bank", body, "email", &mut result).await;

        assert!(fired);
        assert_eq!(result.merchant_raw.as_deref(), Some("RAZ*SWIGGY LIMITE"));
        assert_eq!(
            result.amount_minor,
            Some(50000),
            "untaught fields must be left alone"
        );
    }

    #[tokio::test]
    async fn an_amount_rule_parses_into_minor_units() {
        let pool = dummy_migrated_pool().await;
        let body = "INR 1,020.00 debited from your account on 01/07/26";
        {
            let conn = pool.get().await.unwrap();
            let b = body.to_string();
            conn.interact(move |c| {
                seed_rule(
                    c,
                    "HDFC Bank",
                    "amount",
                    &b,
                    serde_json::json!({"regex": r"INR\s+([\d,.]+)\s", "capture_group": 1}),
                    "active",
                )
            })
            .await
            .unwrap();
        }
        let mut result = ExtractionResult::default();
        apply_learned_fields(&pool, "HDFC Bank", body, "email", &mut result).await;
        assert_eq!(result.amount_minor, Some(102000));
    }

    #[tokio::test]
    async fn an_override_applies_only_to_its_own_template() {
        let pool = dummy_migrated_pool().await;
        let taught = "Rs 500 credited to your account on 01/07/26";
        let other = "Your statement for June is ready. Total due Rs 900.";
        {
            let conn = pool.get().await.unwrap();
            let b = taught.to_string();
            conn.interact(move |c| {
                seed_rule(
                    c,
                    "HDFC Bank",
                    "direction",
                    &b,
                    serde_json::json!({"override_value": "credit"}),
                    "active",
                )
            })
            .await
            .unwrap();
        }

        let mut on_template = ExtractionResult {
            direction: Some("debit".to_string()),
            ..Default::default()
        };
        apply_learned_fields(&pool, "HDFC Bank", taught, "email", &mut on_template).await;
        assert_eq!(on_template.direction.as_deref(), Some("credit"));

        let mut off_template = ExtractionResult {
            direction: Some("debit".to_string()),
            ..Default::default()
        };
        apply_learned_fields(&pool, "HDFC Bank", other, "email", &mut off_template).await;
        assert_eq!(
            off_template.direction.as_deref(),
            Some("debit"),
            "an override must never leak to a different template shape"
        );
    }

    #[tokio::test]
    async fn learned_rules_never_cross_banks() {
        let pool = dummy_migrated_pool().await;
        let body = "Rs 500 spent at SWIGGY on 01/07/26";
        {
            let conn = pool.get().await.unwrap();
            let b = body.to_string();
            conn.interact(move |c| {
                seed_rule(
                    c,
                    "HDFC Bank",
                    "merchant",
                    &b,
                    serde_json::json!({"regex": r"at\s+(.{1,80}?)\s+on", "capture_group": 1}),
                    "active",
                )
            })
            .await
            .unwrap();
        }
        let mut result = ExtractionResult::default();
        let fired = apply_learned_fields(&pool, "ICICI Bank", body, "email", &mut result).await;
        assert!(!fired);
        assert!(result.merchant_raw.is_none());
    }

    #[tokio::test]
    async fn a_rule_that_does_not_match_this_body_is_simply_skipped() {
        let pool = dummy_migrated_pool().await;
        let taught = "Rs 500 spent at SWIGGY on 01/07/26";
        {
            let conn = pool.get().await.unwrap();
            let b = taught.to_string();
            conn.interact(move |c| {
                seed_rule(
                    c,
                    "HDFC Bank",
                    "merchant",
                    &b,
                    serde_json::json!({"regex": r"at\s+(.{1,80}?)\s+on", "capture_group": 1}),
                    "active",
                )
            })
            .await
            .unwrap();
        }
        let mut result = ExtractionResult::default();
        let fired = apply_learned_fields(
            &pool,
            "HDFC Bank",
            "A totally different message shape entirely.",
            "email",
            &mut result,
        )
        .await;
        assert!(!fired, "coexistence across templates depends on this");
    }

    #[tokio::test]
    async fn the_layer_returns_none_without_a_complete_result() {
        let pool = dummy_migrated_pool().await;
        let body = "Rs 500 spent at SWIGGY on 01/07/26";
        {
            let conn = pool.get().await.unwrap();
            let b = body.to_string();
            conn.interact(move |c| {
                seed_rule(
                    c,
                    "HDFC Bank",
                    "merchant",
                    &b,
                    serde_json::json!({"regex": r"at\s+(.{1,80}?)\s+on", "capture_group": 1}),
                    "active",
                )
            })
            .await
            .unwrap();
        }
        assert!(LearnedFieldLayer
            .extract(&pool, "HDFC Bank", body)
            .await
            .is_none());
    }

    #[tokio::test]
    async fn drift_is_not_declared_for_an_unknown_template() {
        let pool = dummy_migrated_pool().await;
        let conn = pool.get().await.unwrap();
        let drift = conn
            .interact(|c| detect_pattern_drift(c, "HDFC Bank", "a body never seen before", &None))
            .await
            .unwrap()
            .unwrap();
        assert!(!drift.drift_detected);
    }

    #[tokio::test]
    async fn drift_is_declared_when_a_known_template_stops_extracting() {
        let pool = dummy_migrated_pool().await;
        let body = "Rs 500 spent at SWIGGY on 01/07/26";
        let conn = pool.get().await.unwrap();
        let b = body.to_string();
        let drift = conn
            .interact(move |c| {
                seed_rule(
                    c,
                    "HDFC Bank",
                    "merchant",
                    &b,
                    serde_json::json!({"regex": r"at\s+(.{1,80}?)\s+on", "capture_group": 1}),
                    "active",
                );
                detect_pattern_drift(c, "HDFC Bank", &b, &None)
            })
            .await
            .unwrap()
            .unwrap();
        assert!(
            drift.drift_detected,
            "rules exist for this shape yet nothing extracted"
        );
    }

    #[tokio::test]
    async fn a_successful_extraction_is_never_drift() {
        let pool = dummy_migrated_pool().await;
        let body = "Rs 500 spent at SWIGGY on 01/07/26";
        let conn = pool.get().await.unwrap();
        let b = body.to_string();
        let drift = conn
            .interact(move |c| {
                seed_rule(
                    c,
                    "HDFC Bank",
                    "merchant",
                    &b,
                    serde_json::json!({"regex": r"at\s+(.{1,80}?)\s+on", "capture_group": 1}),
                    "active",
                );
                detect_pattern_drift(c, "HDFC Bank", &b, &Some(ExtractionResult::default()))
            })
            .await
            .unwrap()
            .unwrap();
        assert!(!drift.drift_detected);
    }

    #[tokio::test]
    async fn drift_does_not_see_a_statement_rule_as_a_known_email_template() {
        let pool = dummy_migrated_pool().await;
        let body = "Rs 500 spent at SWIGGY on 01/07/26";
        let conn = pool.get().await.unwrap();
        let b = body.to_string();
        let drift = conn
            .interact(move |c| {
                let now = chrono::Utc::now().naive_utc();
                crate::db::field_rules::upsert_variant(
                    c,
                    &crate::db::field_rules::FieldRuleVariant {
                        id: "pdf_rule".to_string(),
                        bank_name: "HDFC Bank".to_string(),
                        field_name: "merchant".to_string(),
                        source_type: "statement_pdf".to_string(),
                        template_hash: compute_template_hash(&b),
                        rule_payload_json: serde_json::json!({
                            "regex": "(.+)", "capture_group": 1
                        }),
                        status: "active".to_string(),
                        success_count: 5,
                        failure_count: 0,
                        confidence: 1.0,
                        authored_by: "deterministic".to_string(),
                        learned_from: "user_edit".to_string(),
                        created_at: Some(now),
                        updated_at: Some(now),
                    },
                    None,
                )
                .unwrap();
                detect_pattern_drift(c, "HDFC Bank", &b, &None)
            })
            .await
            .unwrap()
            .unwrap();
        assert!(!drift.drift_detected);
    }

    #[tokio::test]
    async fn test_learned_rule_applied_when_active() {
        let pool = setup_db_with_rule("active".to_string()).await;
        let layer = LearnedFieldLayer;
        let body = "Your amount is 1500 INR at Amazon debit time 1700000000";

        let result = layer.extract(&pool, "Chase", body).await;

        assert!(result.is_some());
        let res = result.unwrap();
        assert_eq!(res.amount_minor, Some(150000));
        assert_eq!(res.merchant_raw, Some("Amazon".to_string()));
        assert_eq!(res.currency, Some("INR".to_string()));
        assert_eq!(res.direction, Some("debit".to_string()));
        assert_eq!(res.extraction_method, "learned_fields");
    }

    #[tokio::test]
    async fn test_learned_rule_matches_across_different_templates() {
        let old_body = "Your amount is 1500 INR at Amazon debit time 1700000000";
        let pool = setup_db_with_rule("active".to_string()).await;

        let new_body =
            "Reminder: your amount is 1500 INR at Amazon debit time 1700000000 -- thank you.";
        assert_ne!(
            compute_template_hash(old_body),
            compute_template_hash(new_body),
            "the two bodies must hash differently to actually exercise cross-template matching"
        );

        let layer = LearnedFieldLayer;
        let result = layer.extract(&pool, "Chase", new_body).await;

        assert!(
            result.is_some(),
            "variants learned from one template must still be tried against a different template's email"
        );
        let res = result.unwrap();
        assert_eq!(res.amount_minor, Some(150000));
        assert_eq!(res.merchant_raw, Some("Amazon".to_string()));
        assert_eq!(res.direction, Some("debit".to_string()));
    }

    #[tokio::test]
    async fn test_inactive_rule_skipped() {
        let pool = setup_db_with_rule("inactive".to_string()).await;
        let layer = LearnedFieldLayer;
        let body = "Your amount is 1500 INR at Amazon debit time 1700000000";

        let result = layer.extract(&pool, "Chase", body).await;

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_pending_rule_not_auto_applied() {
        let pool = setup_db_with_rule("pending".to_string()).await;
        let layer = LearnedFieldLayer;
        let body = "Your amount is 1500 INR at Amazon debit time 1700000000";

        let result = layer.extract(&pool, "Chase", body).await;

        assert!(
            result.is_none(),
            "a pending rule must not be auto-applied, even when its regex matches"
        );
    }

    #[tokio::test]
    async fn test_hdfc_credit_card_regex() {
        let pool = dummy_pool();
        let layer = BankTemplateLayer;
        let body =
            "Rs 1500.00 spent on your HDFC Bank CREDIT Card ending 1234 at Amazon on 25-May-23.";
        let result = layer.extract(&pool, "HDFC Bank", body).await.unwrap();
        assert_eq!(result.amount_minor, Some(150000));
        assert_eq!(result.merchant_raw, Some("Amazon".to_string()));
        assert!(result.event_time.is_some());
        assert_eq!(result.extraction_method, "bank_templates");

        let body_4_digit =
            "Rs 1500.00 spent on your HDFC Bank CREDIT Card ending 1234 at Amazon on 25-May-2023.";
        let result_4 = layer
            .extract(&pool, "HDFC Bank", body_4_digit)
            .await
            .unwrap();
        assert_eq!(result_4.amount_minor, Some(150000));
        assert_eq!(result_4.event_time, Some(ymd_ts(2023, 5, 25)));
    }

    #[tokio::test]
    async fn test_bank_template_invalid_date_no_fallback_leaves_event_time_none() {
        let pool = dummy_pool();
        let layer = BankTemplateLayer;
        let body =
            "Rs 1500.00 spent on your HDFC Bank CREDIT Card ending 1234 at Amazon on 35-May-23.";
        let result = layer.extract(&pool, "HDFC Bank", body).await.unwrap();
        assert_eq!(
            result.event_time, None,
            "an invalid date with no configured fallback must not fabricate a timestamp"
        );
        assert!(
            !result.is_valid(),
            "a result with no event_time must fail is_valid(), which is what makes the \
             orchestrator correctly skip past this layer instead of accepting a corrupted date"
        );
    }

    #[tokio::test]
    async fn test_hdfc_debit_card_regex() {
        let pool = dummy_pool();
        let layer = BankTemplateLayer;
        let body = "Rs 500.00 debited from HDFC Bank A/c ending 1234 at Amazon on 25-May-23";
        let result = layer.extract(&pool, "HDFC Bank", body).await.unwrap();
        assert_eq!(result.amount_minor, Some(50000));
        assert_eq!(result.merchant_raw, Some("Amazon".to_string()));
        assert_eq!(
            result.direction,
            Some("debit".to_string()),
            "debit-shaped pattern must resolve to debit direction"
        );
    }

    #[tokio::test]
    async fn test_hdfc_credit_pattern_resolves_credit_direction_not_hardcoded_debit() {
        let pool = dummy_pool();
        let layer = BankTemplateLayer;
        let body =
            "Rs 5000.00 credited to your HDFC Bank A/c ending 1234 from John Doe on 25-May-23";
        let result = layer.extract(&pool, "HDFC Bank", body).await.unwrap();
        assert_eq!(result.amount_minor, Some(500000));
        assert_eq!(
            result.direction,
            Some("credit".to_string()),
            "a credit-shaped template match must not be mislabeled debit"
        );
        assert_eq!(result.extraction_method, "bank_templates");
    }

    #[tokio::test]
    async fn test_icici_credit_card_regex() {
        let pool = dummy_pool();
        let layer = BankTemplateLayer;
        let body = "INR 1500.00 spent on ICICI Bank Card XX1234 on 25-May-23 at Amazon.";
        let result = layer.extract(&pool, "ICICI Bank", body).await.unwrap();
        assert_eq!(result.amount_minor, Some(150000));
        assert_eq!(result.merchant_raw, Some("Amazon".to_string()));
    }

    #[tokio::test]
    async fn test_icici_upi_regex() {
        let pool = dummy_pool();
        let layer = BankTemplateLayer;
        let body = "Dear Customer, Acct XX1234 debited with INR 500.00 on 25-May-23. Info: UPI/1234567890/Amazon.";
        let result = layer.extract(&pool, "ICICI Bank", body).await.unwrap();
        assert_eq!(result.amount_minor, Some(50000));
        assert_eq!(result.merchant_raw, Some("Amazon".to_string()));
    }

    #[tokio::test]
    async fn test_sbi_credit_card_regex() {
        let pool = dummy_pool();
        let layer = BankTemplateLayer;
        let body = "Rs 1500.00 spent on your SBI Credit Card ending 1234 at Amazon on 25-May-23.";
        let result = layer
            .extract(&pool, "State Bank of India", body)
            .await
            .unwrap();
        assert_eq!(result.amount_minor, Some(150000));
        assert_eq!(result.merchant_raw, Some("Amazon".to_string()));
    }

    #[tokio::test]
    async fn test_axis_credit_card_regex() {
        let pool = dummy_pool();
        let layer = BankTemplateLayer;
        let body = "Rs 1500.00 spent on your Axis Bank Credit Card XX1234 at Amazon on 25-May-23.";
        let result = layer.extract(&pool, "Axis Bank", body).await.unwrap();
        assert_eq!(result.amount_minor, Some(150000));
        assert_eq!(result.merchant_raw, Some("Amazon".to_string()));
    }

    #[tokio::test]
    async fn test_kotak_credit_card_regex() {
        let pool = dummy_pool();
        let layer = BankTemplateLayer;
        let body = "Rs 1500.00 spent on your Kotak Mahindra Bank Credit Card XX1234 at Amazon on 25-May-23.";
        let result = layer
            .extract(&pool, "Kotak Mahindra Bank", body)
            .await
            .unwrap();
        assert_eq!(result.amount_minor, Some(150000));
        assert_eq!(result.merchant_raw, Some("Amazon".to_string()));
    }

    #[tokio::test]
    async fn test_yes_bank_credit_card_regex() {
        let pool = dummy_pool();
        let layer = BankTemplateLayer;
        let body = "Rs 1500.00 spent on your YES Bank Credit Card XX1234 at Amazon on 25-May-23.";
        let result = layer.extract(&pool, "Yes Bank", body).await.unwrap();
        assert_eq!(result.amount_minor, Some(150000));
        assert_eq!(result.merchant_raw, Some("Amazon".to_string()));
    }
    #[tokio::test]
    async fn test_generic_regex_fallback_success() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "You have paid Rs 1,500.50 to Zomato via UPI on 25/05/2023. Ref: 123456789012.";
        let result = layer.extract(&pool, "Any Bank", body).await.unwrap();

        assert_eq!(result.amount_minor, Some(150050));
        assert_eq!(result.currency, Some("INR".to_string()));
        assert_eq!(result.direction, Some("debit".to_string()));
        assert_eq!(result.merchant_raw, Some("Zomato".to_string()));
        assert_eq!(result.reference_id, Some("123456789012".to_string()));
        assert!(result.event_time.is_some());
        assert_eq!(result.extraction_method, "generic_regex");
    }

    #[tokio::test]
    async fn test_generic_regex_fallback_failure() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "Random email without proper transaction details.";
        let result = layer.extract(&pool, "Any Bank", body).await;

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_generic_regex_invalid_date_fails_validation_not_fake_date() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "You have paid Rs 500.00 to Zomato via UPI on 99/99/9999. Ref: 123456789012.";
        let result = layer.extract(&pool, "Any Bank", body).await;
        assert!(
            result.is_none(),
            "an unparseable date must fail the layer entirely, not fabricate a fake timestamp"
        );
    }

    #[test]
    fn test_date_parsers_return_none_not_fake_sentinel_on_failure() {
        assert_eq!(parse_date_generic("not a date"), None);
        assert_eq!(parse_date_generic("35-May-23"), None);
        assert_eq!(parse_date_generic("32/13/26"), None);
    }

    #[test]
    fn test_real_bank_date_formats_parse() {
        for (input, y, m, d) in [
            ("23-12-25", 2025, 12, 23),
            ("10/01/26", 2026, 1, 10),
            ("08-JAN-26", 2026, 1, 8),
            ("30-JUL-2025", 2025, 7, 30),
            ("05 AUG 2025", 2025, 8, 5),
            ("29 Nov 2025", 2025, 11, 29),
            ("17-08-2025", 2025, 8, 17),
            ("07 Jan, 2026", 2026, 1, 7),
            ("Jan 08, 2026", 2026, 1, 8),
            ("Mon, Dec 01, 2025", 2025, 12, 1),
            ("29-Dec-25", 2025, 12, 29),
        ] {
            let parsed = parse_date_generic(input)
                .unwrap_or_else(|| panic!("real bank date {input:?} must parse"));
            assert_eq!(
                parsed.timestamp,
                ymd_ts(y, m, d),
                "{input:?} parsed to the wrong day"
            );
        }
    }

    fn ymd_ts(year: i32, month: u32, day: u32) -> i64 {
        chrono::NaiveDate::from_ymd_opt(year, month, day)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp()
    }

    #[test]
    fn test_parse_date_generic_ambiguous_flag() {
        let unambiguous_numeric = parse_date_generic("25/05/2023").unwrap();
        assert!(!unambiguous_numeric.ambiguous);

        let month_name = parse_date_generic("05-Aug-2026").unwrap();
        assert!(!month_name.ambiguous);

        let ambiguous = parse_date_generic("02-07-2026").unwrap();
        assert!(ambiguous.ambiguous);
        assert_eq!(ambiguous.timestamp, ymd_ts(2026, 7, 2));

        let noop_swap = parse_date_generic("05-05-2026").unwrap();
        assert!(!noop_swap.ambiguous);
    }

    #[test]
    fn test_apply_date_cross_check_noop_when_not_flagged_ambiguous() {
        let original_ts = ymd_ts(2026, 8, 5);
        let mut obs = ExtractionResult {
            event_time: Some(original_ts),
            event_time_ambiguous: false,
            ..Default::default()
        };
        let anchor = Some(ymd_ts(2026, 5, 5));

        apply_date_cross_check(&mut obs, anchor);

        assert_eq!(obs.event_time, Some(original_ts));
        assert_eq!(obs.date_cross_check_flag, None);
    }

    #[test]
    fn test_apply_date_cross_check_decisive_swap() {
        let original_ts = ymd_ts(2026, 7, 2);
        let mut obs = ExtractionResult {
            event_time: Some(original_ts),
            event_time_ambiguous: true,
            ..Default::default()
        };
        let anchor = Some(ymd_ts(2026, 2, 7));

        apply_date_cross_check(&mut obs, anchor);

        assert_eq!(obs.event_time, Some(ymd_ts(2026, 2, 7)));
        assert_eq!(
            obs.date_cross_check_flag,
            Some("swapped_by_anchor".to_string())
        );
    }

    #[test]
    fn test_apply_date_cross_check_weak_signal_untouched() {
        let original_ts = ymd_ts(2026, 7, 2);
        let mut obs = ExtractionResult {
            event_time: Some(original_ts),
            event_time_ambiguous: true,
            ..Default::default()
        };
        let anchor = Some(ymd_ts(2026, 7, 3));

        apply_date_cross_check(&mut obs, anchor);

        assert_eq!(obs.event_time, Some(original_ts));
        assert_eq!(obs.date_cross_check_flag, None);
    }

    #[test]
    fn test_apply_date_cross_check_both_implausible_flags_for_review() {
        let original_ts = ymd_ts(2026, 7, 2);
        let mut obs = ExtractionResult {
            event_time: Some(original_ts),
            event_time_ambiguous: true,
            confidence_score: Some(0.6),
            ..Default::default()
        };
        let anchor = Some(ymd_ts(2026, 10, 2));

        apply_date_cross_check(&mut obs, anchor);

        assert_eq!(obs.event_time, Some(original_ts));
        assert_eq!(
            obs.date_cross_check_flag,
            Some("anchor_mismatch_needs_review".to_string())
        );
        assert!(obs.confidence_score.unwrap() <= CROSS_CHECK_DISAGREEMENT_CONFIDENCE);
    }

    #[test]
    fn test_apply_date_cross_check_no_anchor_is_noop() {
        let original_ts = ymd_ts(2026, 7, 2);
        let mut obs = ExtractionResult {
            event_time: Some(original_ts),
            event_time_ambiguous: true,
            ..Default::default()
        };

        apply_date_cross_check(&mut obs, None);

        assert_eq!(obs.event_time, Some(original_ts));
        assert_eq!(obs.date_cross_check_flag, None);
    }

    #[tokio::test]
    async fn test_generic_amount_extraction() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "You have paid Rs 1,500.50 to Zomato via UPI on 25/05/2023.";
        let result = layer.extract(&pool, "Any Bank", body).await.unwrap();
        assert_eq!(result.amount_minor, Some(150050));
        assert_eq!(result.currency, Some("INR".to_string()));
        assert!(
            result.confidence_score.unwrap() > 0.5 && result.confidence_score.unwrap() <= 0.7,
            "Layer 3 confidence must stay within the documented 0.5-0.7 range, below \
             Layer 1/2's 0.9+, got {:?}",
            result.confidence_score
        );
    }

    #[tokio::test]
    async fn test_generic_confidence_varies_by_field_strength() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;

        let strong_body =
            "You have paid Rs 1,500.50 paid to Zomato via UPI on 25/05/2023. Ref: 123456789012.";
        let strong = layer.extract(&pool, "Any Bank", strong_body).await.unwrap();
        assert_eq!(strong.confidence_score, Some(LAYER3_MAX_CONFIDENCE));

        let weak_body = "Rs 500.00 at Zomato on 25/05/2023.";
        let weak = layer.extract(&pool, "Any Bank", weak_body).await.unwrap();
        assert!(
            weak.confidence_score.unwrap() < strong.confidence_score.unwrap(),
            "a weaker extraction must score strictly lower than a strong one, got weak={:?} strong={:?}",
            weak.confidence_score,
            strong.confidence_score
        );
        assert_eq!(
            weak.confidence_score,
            Some(
                LAYER3_BASE_CONFIDENCE
                    + LAYER3_AMOUNT_CURRENCY_BONUS
                    + LAYER3_AMBIGUOUS_MERCHANT_BONUS
            )
        );
    }

    #[tokio::test]
    async fn test_generic_direction_keyword_proximity() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;

        let debit_body = "Rs 500 spent at Amazon on 01-Jan-24.";
        let debit_result = layer.extract(&pool, "Any Bank", debit_body).await.unwrap();
        assert_eq!(debit_result.direction, Some("debit".to_string()));

        let credit_body = "Rs 500 credited to your account from Amazon Refund on 01-Jan-24.";
        let credit_result = layer.extract(&pool, "Any Bank", credit_body).await.unwrap();
        assert_eq!(credit_result.direction, Some("credit".to_string()));
    }

    #[tokio::test]
    async fn test_generic_merchant_heuristic() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "Rs 1,500.50 paid to Zomato via UPI on 25/05/2023.";
        let result = layer.extract(&pool, "Any Bank", body).await.unwrap();
        assert_eq!(result.merchant_raw, Some("Zomato".to_string()));
    }

    #[tokio::test]
    async fn test_generic_merchant_heuristic_towards() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "Rs 250.00 paid towards Swiggy via UPI on 25/05/2023.";
        let result = layer.extract(&pool, "Any Bank", body).await.unwrap();
        assert_eq!(result.merchant_raw, Some("Swiggy".to_string()));
    }

    #[tokio::test]
    async fn test_generic_merchant_heuristic_info_colon() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "Rs 99.00 debited on 25/05/2023. Info: Starbucks Coffee";
        let result = layer.extract(&pool, "Any Bank", body).await.unwrap();
        assert_eq!(result.merchant_raw, Some("Starbucks Coffee".to_string()));
    }

    #[tokio::test]
    async fn test_generic_merchant_heuristic_asterisk_descriptor() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "Rs. 2590.00 has been debited from your HDFC Bank Credit Card ending 0364 towards RAZ*SWIGGY on 24 May, 2026 at 19:34:18 .";
        let result = layer.extract(&pool, "HDFC Bank", body).await.unwrap();
        assert_eq!(result.merchant_raw, Some("RAZ*SWIGGY".to_string()));
        assert_eq!(result.amount_minor, Some(259000));
    }

    #[tokio::test]
    async fn test_generic_date_space_separated_day_month_year() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "Transaction Successful! INR 193.92 spent on your IDFC FIRST BANK Credit Card ending XX1920 at CRED TELECOM on 23 MAY 2026.";
        let result = layer.extract(&pool, "IDFC FIRST Bank", body).await.unwrap();
        assert_eq!(result.merchant_raw, Some("CRED TELECOM".to_string()));
        assert_eq!(result.amount_minor, Some(19392));
        assert!(result.event_time.is_some());
    }

    #[tokio::test]
    async fn test_generic_merchant_heuristic_colon_label_and_month_first_date() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "You've received ₹15563.0 in Federal Bank Savings Account ending with 1527.\nPayment from:                                ADITYA RAWAL\nDate                                May 30, 2026";
        let result = layer.extract(&pool, "Jupiter", body).await.unwrap();
        assert_eq!(result.merchant_raw, Some("ADITYA RAWAL".to_string()));
        assert_eq!(result.amount_minor, Some(1556300));
        assert_eq!(result.direction, Some("credit".to_string()));
        assert!(result.event_time.is_some());
    }

    #[tokio::test]
    async fn test_generic_merchant_heuristic_two_word_label_next_line() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "24-04-2026\n\nDear Customer,\n\nHere's the summary of your successful AutoPay transaction:\n\nTransaction Amount:\n\nINR 0.00\n\nMerchant Name:\n\nScribdInc\n\nAutoPay ID:\n\nYPXvrvJ1jr\n\nAxis Bank Credit Card No.\n\nXX3825\n\nMax Limit:\n\nINR 1000.00\n\nYou'll receive a notification mentioning the transaction amount prior to any subsequent debit initiated by ScribdInc.";
        let result = layer.extract(&pool, "Axis Bank", body).await.unwrap();
        assert_eq!(result.merchant_raw, Some("ScribdInc".to_string()));
        assert_eq!(result.amount_minor, Some(0));
        assert_eq!(result.direction, Some("debit".to_string()));
        assert!(result.event_time.is_some());
    }

    #[tokio::test]
    async fn test_generic_merchant_skips_self_referential_account() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "₹17000.0 was credited to your account\nYou've received ₹17000.0 in Federal Bank Savings Account ending with 1527.\nPayment from:                                ADITYA RAWAL\nDate                                Jun 30, 2026";
        let result = layer.extract(&pool, "Jupiter", body).await.unwrap();
        assert_eq!(result.merchant_raw, Some("ADITYA RAWAL".to_string()));
        assert_eq!(result.amount_minor, Some(1700000));
    }

    #[tokio::test]
    async fn test_generic_regex_underscore_merchant_and_disclaimer_footer() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "Dear Customer, Greetings from YES BANK. INR 91.00 has been spent on your YES BANK Credit Card ending with 2982 at UPI_SRI SAI FRUITS AND on 10-07-2026 at 08:55:35 pm. Avl Bal INR 82434.42. In case, this transaction was not initiated by you, please block your card immediately by calling our 24x7 customer care or visiting the nearest branch.";
        let result = layer.extract(&pool, "Yes Bank", body).await.unwrap();
        assert_eq!(
            result.merchant_raw,
            Some("UPI_SRI SAI FRUITS AND".to_string())
        );
        assert_eq!(result.direction, Some("debit".to_string()));
        assert_eq!(result.amount_minor, Some(9100));
    }

    #[tokio::test]
    async fn test_generic_merchant_rejects_stopword_only_disclaimer_capture() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body =
            "INR 250.00 debited. To block your card, SMS BLOCK to 9876543210 or call our helpline.";
        let result = layer.extract(&pool, "Yes Bank", body).await;
        if let Some(r) = result {
            assert_ne!(r.merchant_raw, Some("block your".to_string()));
        }
    }

    #[tokio::test]
    async fn test_generic_amount_recognizes_spelled_out_iso_currency_code() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "A transaction of USD 1.00 on your YES BANK Credit Card ending 2982 on 20-05-2026 at 11:57:54 pm at OPENAI is declined because International Ecom/online transactions are disabled on your card.";
        let result = layer.extract(&pool, "Yes Bank", body).await.unwrap();
        assert_eq!(result.amount_minor, Some(100));
        assert_eq!(result.currency, Some("USD".to_string()));
    }

    #[tokio::test]
    async fn test_generic_merchant_terminates_before_declined_prose() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "A transaction of USD 1.00 on your YES BANK Credit Card ending 2982 on 20-05-2026 at 11:57:54 pm at OPENAI is declined because International Ecom/online transactions are disabled on your card. To enable,please visit iris by YES BANK app.";
        let result = layer.extract(&pool, "Yes Bank", body).await.unwrap();
        assert_eq!(result.merchant_raw, Some("OPENAI".to_string()));
    }

    #[test]
    fn test_generic_date_fallback_to_internal_date() {
        use crate::ingestion::message_processor::MessageProcessor;

        assert_eq!(MessageProcessor::internal_date_fallback(&None), None);

        let internal_date = Some("1700000000000".to_string());
        assert_eq!(
            MessageProcessor::internal_date_fallback(&internal_date),
            Some(1_700_000_000)
        );

        let malformed = Some("not-a-number".to_string());
        assert_eq!(MessageProcessor::internal_date_fallback(&malformed), None);
    }

    #[test]
    fn test_amount_minor_converter_indian_formatting() {
        assert_eq!(parse_amount("1,00,000.00"), Some(10000000));
        assert_eq!(parse_amount("1,500.50"), Some(150050));
        assert_eq!(parse_amount("500"), Some(50000));
        assert_eq!(parse_amount("10,00,00,000"), Some(10000000000));
    }

    #[tokio::test]
    async fn test_nlp_parser_hdfc_debit_alert() {
        let pool = dummy_pool();
        let layer = NlpLayer;
        let body = "Rs 500.00 debited from HDFC Bank A/c ending 1234 at Amazon on 25-May-23 Bal Rs 1000.00";
        let result = layer.extract(&pool, "HDFC Bank", body).await.unwrap();

        assert_eq!(result.amount_minor, Some(50000));
        assert_eq!(result.currency, Some("INR".to_string()));
        assert_eq!(result.direction, Some("debit".to_string()));
        assert_eq!(result.merchant_raw, Some("Amazon".to_string()));
        assert_eq!(result.balance_after, Some(100000));
        assert!(result.event_time.is_some());
        assert_eq!(result.extraction_method, "nlp");
    }

    #[tokio::test]
    async fn test_nlp_parser_upi_alert_with_vpa() {
        let pool = dummy_pool();
        let layer = NlpLayer;
        let body = "Dear Customer, Acct XX1234 debited with INR 500.00 on 25-May-23. Info: UPI/1234567890/AmazonPay.";
        let result = layer.extract(&pool, "Any Bank", body).await.unwrap();

        assert_eq!(result.amount_minor, Some(50000));
        assert_eq!(result.currency, Some("INR".to_string()));
        assert_eq!(result.direction, Some("debit".to_string()));
        assert_eq!(result.merchant_raw, Some("AmazonPay".to_string()));
        assert!(result.event_time.is_some());
        assert_eq!(result.extraction_method, "nlp");
    }

    #[tokio::test]
    async fn test_nlp_strict_label_rescue_finds_merchant_ambiguous_tier_would_miss() {
        let pool = dummy_pool();
        let layer = NlpLayer;
        let body = "Rs 500.00 debited towards Zomato on 25-May-23";
        let result = layer
            .extract(&pool, "Any Bank", body)
            .await
            .expect("must extract successfully via the strict-label rescue");
        assert_eq!(result.merchant_raw, Some("Zomato".to_string()));
        assert_eq!(result.direction, Some("debit".to_string()));
        assert_eq!(result.amount_minor, Some(50000));
    }

    #[tokio::test]
    async fn test_nlp_first_valid_merchant_not_overwritten_by_later_disclaimer() {
        let pool = dummy_pool();
        let layer = NlpLayer;
        let body = "Rs 500.00 debited from HDFC Bank A/c ending 1234 at Amazon on 25-May-23 Bal Rs 1000.00. To block your card immediately, call our helpline.";
        let result = layer.extract(&pool, "HDFC Bank", body).await.unwrap();

        assert_eq!(result.merchant_raw, Some("Amazon".to_string()));
    }

    #[test]
    fn test_instrument_signals_credit_card_last4() {
        let body =
            "Rs 1500.00 spent on your HDFC Bank CREDIT Card ending 1234 at Amazon on 25-May-23.";
        let signals = extract_instrument_signals("HDFC Bank", body);
        assert_eq!(signals.masked_identifier, Some("1234".to_string()));
        assert_eq!(signals.instrument_type, Some("credit_card".to_string()));
        assert_eq!(signals.issuer_name, Some("HDFC Bank".to_string()));
    }

    #[test]
    fn test_instrument_signals_bank_account_suffix() {
        let body = "Rs 500.00 debited from HDFC Bank A/c ending 5678 at Amazon on 25-May-23.";
        let signals = extract_instrument_signals("HDFC Bank", body);
        assert_eq!(signals.masked_identifier, Some("5678".to_string()));
        assert_eq!(signals.instrument_type, Some("bank_account".to_string()));
        assert_eq!(signals.issuer_name, Some("HDFC Bank".to_string()));
    }

    #[test]
    fn test_instrument_signals_edge_cases() {
        let s1 = extract_instrument_signals("Bank", "card ending XXXX1234");
        assert_eq!(s1.masked_identifier, Some("1234".to_string()));

        let s2 = extract_instrument_signals("Bank", "card ending XXXXXX1234");
        assert_eq!(s2.masked_identifier, Some("1234".to_string()));

        let s3 = extract_instrument_signals("Bank", "card ending 1234");
        assert_eq!(s3.masked_identifier, Some("1234".to_string()));

        let s4 = extract_instrument_signals("Bank", "card ending XXXX34");
        assert_eq!(s4.masked_identifier, Some("34".to_string()));

        let s5 = extract_instrument_signals("Bank", "account XXXX 1234");
        assert_eq!(s5.masked_identifier, Some("1234".to_string()));

        let s6 = extract_instrument_signals("Bank", "card ending XXXX XXXX 1234");
        assert_eq!(s6.masked_identifier, Some("1234".to_string()));

        let s7 = extract_instrument_signals("Bank", "card ending **** **** **** 1234");
        assert_eq!(s7.masked_identifier, Some("1234".to_string()));

        let s8 = extract_instrument_signals("Bank", "account no. XX-1234");
        assert_eq!(s8.masked_identifier, Some("1234".to_string()));
    }

    #[test]
    fn test_instrument_signals_upi_vpa_detected() {
        let body = "Dear Customer, UPI payment of Rs 200 credited to your VPA user@icici from merchant@upi on 25-May-23.";
        let signals = extract_instrument_signals("ICICI Bank", body);
        assert_eq!(signals.masked_identifier, Some("user@icici".to_string()));
        assert_eq!(signals.instrument_type, Some("upi_vpa".to_string()));
        assert_eq!(signals.upi_vpa, Some("user@icici".to_string()));
        assert_eq!(signals.issuer_name, Some("ICICI Bank".to_string()));
    }

    #[test]
    fn test_instrument_signals_counterparty_vpa_ignored_for_user_instrument() {
        let body = "Dear Customer, Rs.750.00 is debited from your account ending 4691 towards VPA 8127696200@jupiteraxis (ADITYA RAWAL) on 07-06-26.";
        let signals = extract_instrument_signals("HDFC Bank", body);
        assert_eq!(signals.masked_identifier, Some("4691".to_string()));
        assert_eq!(signals.instrument_type, Some("bank_account".to_string()));
        assert_eq!(signals.upi_vpa, None);
    }

    #[test]
    fn test_instrument_signals_network_detected() {
        let body =
            "Rs 1500.00 spent on your Axis Visa Credit Card ending 9999 at Flipkart on 01-Jan-24.";
        let signals = extract_instrument_signals("Axis Bank", body);
        assert_eq!(signals.network, Some("Visa".to_string()));
        assert_eq!(signals.masked_identifier, Some("9999".to_string()));
    }

    #[test]
    fn test_instrument_signals_no_match_returns_only_issuer() {
        let body = "Newsletter from your bank. No transaction details.";
        let signals = extract_instrument_signals("SBI", body);
        assert!(signals.masked_identifier.is_none());
        assert!(signals.instrument_type.is_none());
        assert_eq!(signals.issuer_name, Some("SBI".to_string()));
        assert!(signals.network.is_none());
    }

    #[test]
    fn test_instrument_signals_jupiter_debit_vpa_extraction() {
        let body = "Hey, Aditya\nYour UPI payment was successful\n\nYou paid ₹14000\n\nPaid to T Jyoshna\n7674036967@ybl\n\nDate Jul 05, 2026\n\nFrom Aditya\n8127696200@jupiteraxis\n\nTransaction ID 1321783237916267118\n\nBank reference Number 699841171866";
        let signals = extract_instrument_signals("Jupiter", body);
        assert_eq!(signals.upi_vpa, Some("8127696200@jupiteraxis".to_string()));
        assert_eq!(
            signals.masked_identifier,
            Some("8127696200@jupiteraxis".to_string())
        );
        assert_eq!(signals.instrument_type, Some("upi_vpa".to_string()));
    }

    #[test]
    fn test_instrument_signals_payee_vpa_only_never_saved_as_user_instrument() {
        let body = "You paid ₹1958.00 to MAX SUPER SPECIALITY HOSPITAL saharahospital.42752193@hdfcbank on 08-Jun-26.";
        let signals = extract_instrument_signals("Jupiter", body);
        assert_eq!(signals.upi_vpa, None);
        assert_eq!(signals.masked_identifier, None);
        assert_eq!(signals.instrument_type, None);
    }

    #[tokio::test]
    async fn test_ladder_augments_result_with_instrument_signals() {
        let pool = dummy_pool();
        let body =
            "Rs 1500.00 spent on your HDFC Bank CREDIT Card ending 1234 at Amazon on 25-May-23.";
        let mut layer6_timed_out = false;
        let result = run_extraction_ladder(
            &pool,
            "HDFC Bank",
            body,
            None,
            false,
            None,
            &mut layer6_timed_out,
            None,
            &mut crate::logging::EmailTrace::new("test"))
        .await
        .unwrap();
        assert!(result.is_some());
        let obs = result.unwrap();
        assert_eq!(obs.amount_minor, Some(150000));
        assert_eq!(obs.masked_identifier, Some("1234".to_string()));
        assert_eq!(obs.instrument_type, Some("credit_card".to_string()));
        assert_eq!(obs.issuer_name, Some("HDFC Bank".to_string()));
    }

    async fn setup_crossref_db(
        entries: Vec<crate::db::statement_entries::StatementEntriesRow>,
    ) -> Pool {
        let pool = dummy_migrated_pool().await;
        let conn = pool.get().await.unwrap();
        conn.interact(move |c| {
            c.execute("INSERT INTO local_profile (id) VALUES (1)", [])
                .unwrap();
            c.execute(
                "INSERT INTO instruments (id, type, issuer_name, masked_identifier, status) \
                 VALUES ('inst_1', 'credit_card', 'HDFC Bank', '1234', 'active')",
                [],
            )
            .unwrap();
            c.execute(
                "INSERT INTO statements (id, instrument_id, statement_type, billing_period_start, billing_period_end, parse_status) \
                 VALUES ('stmt_1', 'inst_1', 'credit_card', '2023-05-01', '2023-05-31', 'parsed')",
                [],
            )
            .unwrap();
            for entry in entries {
                crate::db::statement_entries::insert(c, &entry).unwrap();
            }
        })
        .await
        .unwrap();
        pool
    }

    fn crossref_entry(
        id: &str,
        transaction_date: chrono::NaiveDate,
        amount_minor: i64,
        reference_id: Option<&str>,
    ) -> crate::db::statement_entries::StatementEntriesRow {
        crate::db::statement_entries::StatementEntriesRow {
            id: id.to_string(),
            statement_id: Some("stmt_1".to_string()),
            row_index: Some(1),
            transaction_date: Some(transaction_date),
            posting_date: None,
            description_raw: Some("AMAZON PAY".to_string()),
            merchant_raw: Some("Amazon".to_string()),
            merchant_normalized: Some("amazon".to_string()),
            amount: Some(amount_minor as f64 / 100.0),
            amount_minor: Some(amount_minor),
            currency: Some("INR".to_string()),
            direction: Some("debit".to_string()),
            reference_id: reference_id.map(|s| s.to_string()),
            location: None,
            raw_row_json: None,
            created_at: None,
        }
    }

    #[tokio::test]
    async fn test_layer5_single_match_completes_extraction() {
        let anchor = chrono::NaiveDate::from_ymd_opt(2023, 5, 25).unwrap();
        let entry_date = chrono::NaiveDate::from_ymd_opt(2023, 5, 24).unwrap();
        let pool = setup_crossref_db(vec![crossref_entry(
            "se_1",
            entry_date,
            150000,
            Some("123456789012"),
        )])
        .await;

        let body = "Rs 1500.00 spent on your HDFC Bank credit card ending 1234.";
        let result = Layer5CrossrefLayer
            .extract(&pool, "HDFC Bank", body, Some(anchor))
            .await;

        assert!(result.is_some());
        let obs = result.unwrap();
        assert_eq!(obs.amount_minor, Some(150000));
        assert_eq!(obs.merchant_raw, Some("Amazon".to_string()));
        assert_eq!(obs.reference_id, Some("123456789012".to_string()));
        assert_eq!(obs.extraction_method, "layer5_statement_crossref");
        assert_eq!(obs.masked_identifier, Some("1234".to_string()));
    }

    #[tokio::test]
    async fn test_layer5_ambiguous_match_returns_none() {
        let anchor = chrono::NaiveDate::from_ymd_opt(2023, 5, 25).unwrap();
        let entry_date = chrono::NaiveDate::from_ymd_opt(2023, 5, 24).unwrap();
        let pool = setup_crossref_db(vec![
            crossref_entry("se_1", entry_date, 150000, Some("111111111111")),
            crossref_entry("se_2", entry_date, 150000, Some("222222222222")),
        ])
        .await;

        let body = "Rs 1500.00 spent on your HDFC Bank credit card ending 1234.";
        let result = Layer5CrossrefLayer
            .extract(&pool, "HDFC Bank", body, Some(anchor))
            .await;

        assert!(
            result.is_none(),
            "two equally-plausible candidates must not be auto-resolved"
        );
    }

    #[tokio::test]
    async fn test_layer5_no_match_returns_none() {
        let anchor = chrono::NaiveDate::from_ymd_opt(2023, 5, 25).unwrap();
        let far_date = chrono::NaiveDate::from_ymd_opt(2023, 6, 10).unwrap();
        let pool = setup_crossref_db(vec![crossref_entry(
            "se_1",
            far_date,
            150000,
            Some("123456789012"),
        )])
        .await;

        let body = "Rs 1500.00 spent on your HDFC Bank credit card ending 1234.";
        let result = Layer5CrossrefLayer
            .extract(&pool, "HDFC Bank", body, Some(anchor))
            .await;

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_layer5_no_anchor_date_returns_none() {
        let pool = setup_crossref_db(vec![]).await;
        let body = "Rs 1500.00 spent on your HDFC Bank credit card ending 1234.";
        let result = Layer5CrossrefLayer
            .extract(&pool, "HDFC Bank", body, None)
            .await;
        assert!(result.is_none());
    }

    async fn setup_drift_db(bank_name: &str, body_to_register: &str) -> (Pool, String) {
        let pool = dummy_migrated_pool().await;
        let template_hash = compute_template_hash(body_to_register);
        let registered_body = body_to_register.to_string();
        let bank_name_str = bank_name.to_string();

        let conn = pool.get().await.unwrap();
        conn.interact(move |c| {
            seed_rule(
                c,
                &bank_name_str,
                "amount",
                &registered_body,
                serde_json::json!({ "regex": r"Rs ([\d,]+) spent", "capture_group": 1 }),
                "active",
            );
        })
        .await
        .unwrap();

        (pool, template_hash)
    }

    #[tokio::test]
    async fn test_drift_detected_for_changed_hdfc_template() {
        let original_body =
            "Rs 1500 spent on HDFC Bank CREDIT Card ending 1234 at Amazon on 25-May-23.";
        let (_pool, registered_hash) = setup_drift_db("HDFC Bank", original_body).await;

        let changed_body =
            "HDFC Bank: Transaction of INR 1500 done at merchant Amazon on 25-May-2023. New format.";
        let changed_hash = compute_template_hash(changed_body);
        assert_ne!(
            registered_hash, changed_hash,
            "Changed body must produce a different template hash to simulate drift"
        );

        let conn = crate::db::test_helpers::setup_test_db_async().await;

        let drift_new_template =
            detect_pattern_drift(&conn, "HDFC Bank", changed_body, &None).unwrap();
        assert!(
            !drift_new_template.drift_detected,
            "A genuinely new (never-seen) template must NOT be flagged as drift; \
             got drift_detected = true"
        );
        assert_eq!(drift_new_template.template_hash, changed_hash);

        seed_rule(
            &conn,
            "HDFC Bank",
            "amount",
            original_body,
            serde_json::json!({ "regex": r"Rs ([\d,]+) spent", "capture_group": 1 }),
            "active",
        );

        let drift_known_template =
            detect_pattern_drift(&conn, "HDFC Bank", original_body, &None).unwrap();
        assert!(
            drift_known_template.drift_detected,
            "Known template (active rules exist) + ladder returned None must be drift; \
             got drift_detected = false"
        );
        assert_eq!(drift_known_template.template_hash, registered_hash);

        let successful_result = Some(ExtractionResult {
            amount_minor: Some(150000),
            currency: Some("INR".to_string()),
            direction: Some("debit".to_string()),
            event_time: Some(1704067200),
            merchant_raw: Some("Amazon".to_string()),
            extraction_method: "bank_templates".to_string(),
            ..Default::default()
        });
        let drift_on_success =
            detect_pattern_drift(&conn, "HDFC Bank", original_body, &successful_result).unwrap();
        assert!(
            !drift_on_success.drift_detected,
            "When the ladder succeeds, drift must never be flagged; \
             got drift_detected = true"
        );
    }

    #[tokio::test]
    async fn test_fx_transaction_extracted_correctly() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "Acct XX1234 debited USD 50.00 (INR 4150.50) on 25-May-23 at Netflix.";
        let result = layer.extract(&pool, "Any Bank", body).await;
        assert!(result.is_some());
    }

    #[tokio::test]
    async fn test_declined_transaction_rejected_or_flagged() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "Transaction of INR 500.00 at POS declined due to insufficient funds.";
        let result = layer.extract(&pool, "Any Bank", body).await;
        assert!(result.is_none() || result.unwrap().amount_minor.unwrap_or(0) > 0);
    }

    #[tokio::test]
    async fn test_multi_amount_format_picks_correct_amount() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "Spent INR 500.00. Available limit is INR 45,000.00.";
        let result = layer.extract(&pool, "Any Bank", body).await;
        if let Some(res) = result {
            assert_eq!(res.amount_minor, Some(50000));
        }
    }

    #[tokio::test]
    async fn test_icici_upi_on_credit_card_regex() {
        let pool = dummy_pool();
        let layer = BankTemplateLayer;
        let body = "Dear Customer, Credit Card XX1234 debited with INR 500.00 on 25-May-23. Info: UPI/1234567890/Amazon.";
        let result = layer.extract(&pool, "ICICI Bank", body).await;
        if let Some(res) = result {
            assert_eq!(res.amount_minor, Some(50000));
        }
    }

    #[test]
    fn test_detect_channel_hdfc_imps_self_transfer() {
        let body =
            "HDFC BANK\n\nDear Customer,\n\nGreetings from HDFC Bank!\n\n INR 1,04,721.00 has \
             been debited from your account ending xxxxxxxxxx4691 on 30-06-26 and credited to the \
             account ending xxxxxxxxxx1527 via IMPS.\n\nIMPS Reference No: 618139547133\nAvailable \
             Balance: INR 10,000.00";
        let obs = ExtractionResult::default();
        assert_eq!(
            detect_channel(&obs, body),
            Some("internal_transfer".to_string())
        );
    }

    #[test]
    fn test_detect_channel_upi_credit_card_requires_credit_card_instrument() {
        let body =
            "Rs 500.00 spent using your Credit Card ending 1234 at Amazon via UPI on 25-May-23.";

        let credit_card_obs = ExtractionResult {
            instrument_type: Some("credit_card".to_string()),
            ..Default::default()
        };
        assert_eq!(
            detect_channel(&credit_card_obs, body),
            Some("upi_credit_card".to_string())
        );

        let bank_account_obs = ExtractionResult::default();
        assert_eq!(
            detect_channel(&bank_account_obs, body),
            Some("upi".to_string())
        );
    }

    #[test]
    fn test_detect_channel_keyword_branches() {
        let obs = ExtractionResult::default();
        let cases: &[(&str, &str)] = &[
            ("Rs 500 debited towards NEFT transfer to XYZ.", "neft"),
            (
                "Rs 50000 transferred via RTGS to account ending 1234.",
                "rtgs",
            ),
            ("Rs 200 spent at POS terminal, Big Bazaar.", "pos"),
            ("Rs 2000 withdrawn from ATM at MG Road.", "atm"),
            ("Rs 150 loaded to your Paytm wallet.", "wallet"),
            ("Cheque no. 123456 cleared for Rs 10000.", "cheque"),
            ("Your NACH mandate for Rs 999 was debited.", "ecs_nach"),
            ("Your BNPL bill of Rs 500 is due.", "bnpl"),
            ("Your loan account has been disbursed Rs 100000.", "loan"),
        ];
        for (body, expected) in cases {
            assert_eq!(
                detect_channel(&obs, body),
                Some(expected.to_string()),
                "body: {body}"
            );
        }
    }

    #[test]
    fn test_detect_channel_emi_fallback_when_no_stronger_signal() {
        let obs = ExtractionResult {
            emi_total_installments: Some(6),
            ..Default::default()
        };
        let body = "Your purchase of Rs 6000 has been converted to EMI, 6 installments.";
        assert_eq!(detect_channel(&obs, body), Some("emi".to_string()));
    }

    #[test]
    fn test_detect_channel_none_when_no_signal_present() {
        let obs = ExtractionResult::default();
        let body = "Rs 500.00 credited to your account from a well-wisher.";
        assert_eq!(detect_channel(&obs, body), None);
    }

    #[tokio::test]
    async fn test_indusind_credit_card_txn_approved() {
        let pool = dummy_pool();
        let layer = BankTemplateLayer;
        let body = "The transaction on your IndusInd Bank Credit Card ending 7480 for INR \
            134.00 on 15-02-2026 09:25:43 pm at Swiggy Limited is Approved. Available Limit: \
            INR 49,866.00.";
        let result = layer
            .extract(&pool, "IndusInd Bank", body)
            .await
            .expect("credit_card_txn_approved pattern must match");
        assert_eq!(result.amount_minor, Some(13400));
        assert_eq!(result.merchant_raw, Some("Swiggy Limited".to_string()));
        assert_eq!(result.direction, Some("debit".to_string()));
    }

    #[tokio::test]
    async fn test_indusind_credit_card_bill_payment_thank_you() {
        let pool = dummy_pool();
        let layer = BankTemplateLayer;
        let body = "Dear Customer,\n\nThank you for your Payment of INR 134.00 towards your \
            IndusInd Bank Credit Card. Your payment is credited to your Credit Card account on \
            15/03/2026.\n\n.";
        let result = layer
            .extract(&pool, "IndusInd Bank", body)
            .await
            .expect("credit_card_bill_payment_thank_you pattern must match");
        assert_eq!(result.amount_minor, Some(13400));
        assert_eq!(
            result.merchant_raw,
            Some("IndusInd Bank Credit Card".to_string())
        );
        assert_eq!(result.direction, Some("credit".to_string()));
        assert!(
            result.masked_identifier.is_none(),
            "this narration genuinely has no card digits anywhere in the source"
        );
    }

    #[tokio::test]
    async fn test_hdfc_neft_transfer_to_payee() {
        let pool = dummy_pool();
        let layer = BankTemplateLayer;
        let body = "Thank you for banking with HDFC Bank.\n\nRs. 70000 has been deducted from \
            your HDFC Bank account ending in XX4691 for a transfer to payee Rina Rawal SBI \
            Account via NEFT using HDFC Bank Online Banking.";
        let result = layer
            .extract(&pool, "HDFC Bank", body)
            .await
            .expect("neft_transfer_to_payee pattern must match");
        assert_eq!(result.amount_minor, Some(7_000_000));
        assert_eq!(
            result.merchant_raw,
            Some("Rina Rawal SBI Account".to_string())
        );
        assert_eq!(result.direction, Some("debit".to_string()));

        let self_transfer_body = "Thank you for banking with HDFC Bank.\n\nRs. 82164 has been \
            deducted from your HDFC Bank account ending in XX4691 for a transfer to payee Self \
            Transfer via NEFT using HDFC Bank Online Banking.";
        let self_result = layer
            .extract(&pool, "HDFC Bank", self_transfer_body)
            .await
            .expect("neft_transfer_to_payee pattern must match the self-transfer wording too");
        assert_eq!(self_result.merchant_raw, Some("Self Transfer".to_string()));
    }

    #[tokio::test]
    async fn test_hdfc_neft_credit_cr_ifsc_name() {
        let pool = dummy_pool();
        let layer = BankTemplateLayer;
        let body = "Greetings from HDFC Bank!\n\nRs.INR 10,000.00 has been successfully added \
            to your account ending XX4691 from NEFT Cr-SBIN0010341-RINA RAWAL-Aditya \
            Rawal-SBIN426064133764 on 05-MAR-2026.";
        let result = layer
            .extract(&pool, "HDFC Bank", body)
            .await
            .expect("neft_credit_cr_ifsc_name pattern must match");
        assert_eq!(result.amount_minor, Some(1_000_000));
        assert_eq!(result.merchant_raw, Some("RINA RAWAL".to_string()));
        assert_eq!(result.direction, Some("credit".to_string()));
    }

    #[tokio::test]
    async fn test_hdfc_account_credit_ref_code_merchant() {
        let pool = dummy_pool();
        let layer = BankTemplateLayer;
        let body = "Greetings from HDFC Bank!\n\nRs.INR 1,12,866.00 has been successfully added \
            to your account ending XX4691 from A2AINT01-THEMATHCOMPANY PRIVATE \
            LIMITED-Salary-SalaryMar26 on 30-MAR-2026.";
        let result = layer
            .extract(&pool, "HDFC Bank", body)
            .await
            .expect("account_credit_ref_code_merchant pattern must match");
        assert_eq!(result.amount_minor, Some(11_286_600));
        assert_eq!(
            result.merchant_raw,
            Some("THEMATHCOMPANY PRIVATE LIMITED".to_string())
        );
        assert_eq!(result.direction, Some("credit".to_string()));
    }

    #[test]
    fn test_parse_amount_trailing_stop_and_implausible_values() {
        assert_eq!(
            parse_amount("706.00."),
            Some(70600),
            "bank prose ends the amount with a full stop; the stray dot must not \
             fail the parse and drop the amount"
        );
        assert_eq!(parse_amount("1,020.00,"), Some(102000));
        assert_eq!(
            parse_amount("99999999999999999999"),
            None,
            "a float-to-int cast saturates, so an out-of-range figure must be \
             rejected rather than booked as i64::MAX paise"
        );
        assert_eq!(
            parse_amount(".50"),
            Some(50),
            "a leading dot is the decimal point"
        );
        assert_eq!(parse_amount("Ref"), None);
        assert_eq!(
            parse_amount("Rs.2500.00"),
            Some(250000),
            "the dot in \"Rs.\" is punctuation, not a decimal point; kept, it \
             leaves \".2500.00\" and the whole figure is dropped"
        );
        assert_eq!(parse_amount("INR.1,020.00"), Some(102000));
    }

    #[tokio::test]
    async fn test_nlp_balance_survives_a_currency_prefix_ending_in_a_dot() {
        let pool = dummy_pool();
        let body = "Rs 500.00 debited from HDFC Bank A/c ending 1234 at Amazon on 25-May-23 \
                    Avl Bal Rs.2500.00";
        let result = NlpLayer.extract(&pool, "HDFC Bank", body).await.unwrap();
        assert_eq!(
            result.balance_after,
            Some(250000),
            "\"Rs.2500.00\" is one token, so the prefix's full stop reaches \
             parse_amount and silently dropped the balance"
        );
    }

    #[test]
    fn test_normalize_direction_abbreviations_are_not_prefixes() {
        for w in ["Crest Hotel", "Cristiano", "Dropbox", "Drone Services"] {
            assert_eq!(
                normalize_direction(w),
                None,
                "{w:?} merely starts with cr/dr; a fabricated direction is worse \
                 than none because the field then looks confidently populated"
            );
        }
        assert_eq!(normalize_direction("CR.").as_deref(), Some("credit"));
        assert_eq!(normalize_direction("dr.").as_deref(), Some("debit"));
    }

    #[test]
    fn test_template_hash_ignores_edge_whitespace() {
        assert_eq!(
            compute_template_hash("Rs 500 spent at SWIGGY"),
            compute_template_hash("\n  Rs 500 spent at SWIGGY  \n\n"),
            "MIME-to-text conversion varies the edge whitespace between two \
             renderings of one template; an unstable hash orphans the overrides \
             taught against it"
        );
    }

    #[tokio::test]
    async fn a_learned_last4_rule_capturing_no_digits_is_dropped() {
        let pool = dummy_migrated_pool().await;
        let body = "Rs 500.00 spent on your Card ending 1234 at Amazon on 01/07/26";
        {
            let conn = pool.get().await.unwrap();
            let b = body.to_string();
            conn.interact(move |c| {
                seed_rule(
                    c,
                    "HDFC Bank",
                    "last4",
                    &b,
                    serde_json::json!({"regex": r"at\s+([A-Za-z]+)", "capture_group": 1}),
                    "active",
                )
            })
            .await
            .unwrap();
        }

        let mut result = ExtractionResult::default();
        let fired = apply_learned_fields(&pool, "HDFC Bank", body, "email", &mut result).await;

        assert!(!fired);
        assert_eq!(
            result.masked_identifier, None,
            "a last4 with no digits and no VPA handle keys a phantom instrument, \
             and it would beat the correctly-read digits because \
             apply_instrument_signals only fills fields still empty"
        );
    }

    #[test]
    fn test_currency_amount_regex_ignores_rs_inside_a_word() {
        let (prefix_re, _) = generic_currency_amount_regexes();
        let caps = prefix_re
            .captures("Your Rewards 500 points. Rs 250.00 debited at Zomato.")
            .expect("the real amount must still match");
        assert_eq!(
            parse_amount(caps.get(2).unwrap().as_str()),
            Some(25000),
            "unanchored, `rs` matches inside \"Rewards\" and a loyalty balance \
             becomes the transaction amount"
        );
        assert!(
            prefix_re
                .captures("Cards 1234 and 5678 are active.")
                .is_none(),
            "a card number is not an amount"
        );
    }

    #[test]
    fn test_debit_card_not_misread_as_credit_card_via_account() {
        let signals = extract_instrument_signals(
            "HDFC Bank",
            "Rs 500.00 spent on your Debit Card ending 1234 from your account on 25-May-23.",
        );
        assert_eq!(
            signals.instrument_type,
            Some("debit_card".to_string()),
            "\"cc\" as a substring hits the \"account\" in every debit-card alert"
        );
    }

    #[test]
    fn test_detect_channel_does_not_invent_channels_from_ordinary_words() {
        let obs = ExtractionResult::default();
        assert_eq!(
            detect_channel(&obs, "Ecstatic news! Rs 500 credited by Nachiket."),
            None,
            "\"ecs\" and \"nach\" as substrings invent a mandate channel out of prose"
        );
    }

    #[tokio::test]
    async fn a_learned_direction_rule_is_normalized_to_the_two_ledger_values() {
        let pool = dummy_migrated_pool().await;
        let body = "Rs 500.00 was credited to your account on 01/07/26";
        {
            let conn = pool.get().await.unwrap();
            let b = body.to_string();
            conn.interact(move |c| {
                seed_rule(
                    c,
                    "HDFC Bank",
                    "direction",
                    &b,
                    serde_json::json!({"regex": "(credited)", "capture_group": 1}),
                    "active",
                );
                seed_rule(
                    c,
                    "HDFC Bank",
                    "currency",
                    &b,
                    serde_json::json!({"regex": r"(Rs)\s", "capture_group": 1}),
                    "active",
                );
            })
            .await
            .unwrap();
        }

        let mut result = ExtractionResult::default();
        apply_learned_fields(&pool, "HDFC Bank", body, "email", &mut result).await;

        assert_eq!(
            result.direction.as_deref(),
            Some("credit"),
            "every consumer compares direction against exactly \"debit\"/\"credit\", \
             so the raw capture \"credited\" matches nothing downstream"
        );
        assert_eq!(
            result.currency.as_deref(),
            Some("INR"),
            "\"Rs\" upper-cased is \"RS\", which is not an ISO code"
        );
    }

    #[tokio::test]
    async fn a_learned_direction_rule_with_unrecognised_wording_is_dropped() {
        let pool = dummy_migrated_pool().await;
        let body = "Rs 500.00 transacted on your account on 01/07/26";
        {
            let conn = pool.get().await.unwrap();
            let b = body.to_string();
            conn.interact(move |c| {
                seed_rule(
                    c,
                    "HDFC Bank",
                    "direction",
                    &b,
                    serde_json::json!({"regex": "(transacted)", "capture_group": 1}),
                    "active",
                )
            })
            .await
            .unwrap();
        }

        let mut result = ExtractionResult::default();
        let fired = apply_learned_fields(&pool, "HDFC Bank", body, "email", &mut result).await;

        assert!(!fired);
        assert_eq!(
            result.direction, None,
            "an unrecognised capture must leave the field untouched rather than \
             writing a value nothing downstream matches"
        );
    }

    #[test]
    fn test_normalize_direction_wordings() {
        for w in ["credited", "CREDIT", "Cr", "credit", "received"] {
            assert_eq!(normalize_direction(w).as_deref(), Some("credit"), "{w}");
        }
        for w in ["debited", "DEBIT", "Dr", "spent", "paid", "withdrawn"] {
            assert_eq!(normalize_direction(w).as_deref(), Some("debit"), "{w}");
        }
        assert_eq!(normalize_direction("transacted"), None);
        assert_eq!(normalize_direction(""), None);
    }

    #[tokio::test]
    async fn test_nlp_first_balance_wins_over_a_later_reward_balance() {
        let pool = dummy_pool();
        let body = "Rs 500.00 debited from HDFC Bank A/c ending 1234 at Amazon on 25-May-23. \
                    Bal: 1000.00. Reward Bal: 0.00";
        let result = NlpLayer.extract(&pool, "HDFC Bank", body).await.unwrap();
        assert_eq!(
            result.balance_after,
            Some(100000),
            "a trailing rewards balance must not overwrite the account balance \
             the message already stated"
        );
    }

    #[tokio::test]
    async fn test_nlp_balance_reads_past_a_run_of_filler_tokens() {
        let pool = dummy_pool();
        let body =
            "Rs 500.00 debited from HDFC Bank A/c ending 1234 at Amazon on 25-May-23 Avl Bal is Rs 2500.00";
        let result = NlpLayer.extract(&pool, "HDFC Bank", body).await.unwrap();
        assert_eq!(
            result.balance_after,
            Some(250000),
            "skipping only one filler token leaves the parse pointed at \"Rs\""
        );
    }

    #[test]
    fn test_card_mask_does_not_bridge_a_sentence_boundary_into_a_date() {
        let signals = extract_instrument_signals(
            "HDFC Bank",
            "Thank you for using your HDFC Bank Credit Card. Transaction on 25-May-23 at Amazon.",
        );
        assert_eq!(
            signals.masked_identifier, None,
            "a sentence-ending full stop is not a mask; \"25\" here would key a \
             phantom instrument"
        );

        // The ellipsis mask the gap exists for must still work.
        assert_eq!(
            extract_instrument_signals("HDFC Bank", "card ending ...1234").masked_identifier,
            Some("1234".to_string())
        );
    }

    #[test]
    fn test_parse_learned_event_time_bounds_and_scales() {
        assert_eq!(
            parse_learned_event_time("123"),
            None,
            "a short numeric capture -- an auth code, an installment count -- \
             must be rejected, not booked as a 1970 event time"
        );
        assert_eq!(parse_learned_event_time("0"), None);

        assert_eq!(
            parse_learned_event_time("1700000000"),
            Some((1_700_000_000, false))
        );
        assert_eq!(
            parse_learned_event_time("1700000000000"),
            Some((1_700_000_000, false)),
            "a millisecond epoch must be rescaled, not read as the year 55000"
        );
        assert_eq!(
            parse_learned_event_time("533264925852"),
            None,
            "a UPI reference number is not a timestamp"
        );
        assert_eq!(
            parse_learned_event_time("2026-03-30 00:00:00"),
            Some((ymd_ts(2026, 3, 30), false)),
            "this is the shape the learning path writes back as a corrected date"
        );
    }

    #[test]
    fn test_iso_dates_parse_without_breaking_day_first_dates() {
        assert_eq!(
            parse_date_generic("2026-03-30").map(|p| p.timestamp),
            Some(ymd_ts(2026, 3, 30))
        );
        assert_eq!(
            parse_date_generic("23-12-25").map(|p| p.timestamp),
            Some(ymd_ts(2025, 12, 23)),
            "ISO parsing must stay last, or this reads as year 23"
        );
    }

    #[tokio::test]
    async fn test_nlp_footer_does_not_flip_direction_or_date() {
        let pool = dummy_pool();
        let body = "Rs 500.00 debited from HDFC Bank A/c ending 1234 at Amazon on 25-May-23 \
                    Bal Rs 1000.00. If the amount is not credited back, report on 01-Jan-24.";
        let result = NlpLayer.extract(&pool, "HDFC Bank", body).await.unwrap();
        assert_eq!(
            result.direction,
            Some("debit".to_string()),
            "a closing disclaimer must not flip the direction the message stated"
        );
        assert_eq!(result.event_time, Some(ymd_ts(2023, 5, 25)));
    }

    #[test]
    fn test_fx_transaction_is_not_flagged_as_amount_disagreement() {
        let body = "Acct XX1234 debited USD 50.00 (INR 4150.50) on 25-May-23 at Netflix.";
        let mut obs = ExtractionResult {
            amount_minor: Some(415050),
            original_amount_minor: Some(5000),
            confidence_score: Some(LAYER12_CONFIDENCE),
            ..Default::default()
        };
        apply_amount_cross_check(&mut obs, body);
        assert_eq!(
            obs.confidence_score,
            Some(LAYER12_CONFIDENCE),
            "the first amount in an FX body is the foreign one; agreeing with it \
             is agreement, not a disagreement to downgrade for"
        );
    }

    #[tokio::test]
    async fn test_layer5_result_carries_a_confidence_score() {
        let anchor = chrono::NaiveDate::from_ymd_opt(2023, 5, 25).unwrap();
        let entry_date = chrono::NaiveDate::from_ymd_opt(2023, 5, 24).unwrap();
        let pool = setup_crossref_db(vec![crossref_entry("se_1", entry_date, 150000, None)]).await;

        let obs = Layer5CrossrefLayer
            .extract(
                &pool,
                "HDFC Bank",
                "Rs 1500.00 spent on your HDFC Bank credit card ending 1234.",
                Some(anchor),
            )
            .await
            .expect("the unique statement entry must complete the extraction");

        assert_eq!(
            obs.confidence_score,
            Some(LAYER5_CONFIDENCE),
            "an unset confidence reads downstream as not confident at all, so a \
             statement-backed result would never auto-resolve"
        );
    }

    #[tokio::test]
    async fn the_rule_authored_for_this_template_wins_over_another_live_variant() {
        let pool = dummy_migrated_pool().await;
        let this_shape = "Rs 500 spent at ALPHA STORE on 01/07/26";
        let other_shape = "Rs 500 spent at BETA STORE on 01/07/26 -- thank you for banking.";

        let conn = pool.get().await.unwrap();
        let (a, b) = (this_shape.to_string(), other_shape.to_string());
        conn.interact(move |c| {
            seed_rule(
                c,
                "HDFC Bank",
                "merchant",
                &a,
                serde_json::json!({"regex": r"at\s+(.{1,80}?)\s+on", "capture_group": 1}),
                "active",
            );
            seed_rule(
                c,
                "HDFC Bank",
                "merchant",
                &b,
                serde_json::json!({"regex": r"spent\s+at\s+(\S+)", "capture_group": 1}),
                "active",
            );
        })
        .await
        .unwrap();
        drop(conn);

        let mut result = ExtractionResult::default();
        apply_learned_fields(&pool, "HDFC Bank", this_shape, "email", &mut result).await;

        assert_eq!(
            result.merchant_raw.as_deref(),
            Some("ALPHA STORE"),
            "both variants match this body, so without a deterministic ranking the \
             winner is whatever order SQLite happened to return"
        );
    }

    #[tokio::test]
    async fn test_hdfc_credit_card_debit_to_upi_handle() {
        let pool = dummy_pool();
        let layer = BankTemplateLayer;
        let body = "Rs.400.00 has been debited from your HDFC Bank RuPay Credit Card XX8256 to \
            paytm-81642725@ptys SUVIM CARE on 22-03-26. Your UPI transaction reference number \
            is 644708657028.";
        let result = layer
            .extract(&pool, "HDFC Bank", body)
            .await
            .expect("credit_card_debit_to_upi_handle pattern must match");
        assert_eq!(result.amount_minor, Some(40000));
        assert_eq!(result.merchant_raw, Some("SUVIM CARE".to_string()));
        assert_eq!(result.direction, Some("debit".to_string()));
        assert_eq!(result.reference_id, Some("644708657028".to_string()));
    }
