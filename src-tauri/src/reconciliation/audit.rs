use anyhow::Result;
use rusqlite::{params, Connection};
use uuid::Uuid;

/// Immutable match decision types (Doc 11 §7).
#[derive(Debug, Clone, PartialEq)]
pub enum DecisionType {
    AutoMatchedExact,
    AutoMatchedScored,
    AmbiguousPending(String),
    NewCanonical,
    ManuallyConfirmed,
    ManuallyCorrected,
    RejectedMatch,
}

impl DecisionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            DecisionType::AutoMatchedExact => "auto_matched_exact",
            DecisionType::AutoMatchedScored => "auto_matched_scored",
            DecisionType::AmbiguousPending(_) => "ambiguous_pending",
            DecisionType::NewCanonical => "new_canonical",
            DecisionType::ManuallyConfirmed => "manually_confirmed",
            DecisionType::ManuallyCorrected => "manually_corrected",
            DecisionType::RejectedMatch => "rejected_match",
        }
    }
}

/// Appends an immutable row to `match_decisions`.
/// Every reconciliation action — auto or manual — must create one of these rows (Doc 11 §7).
/// This table is the auditable, append-only trail of all reconciliation outcomes.
///
/// `reviewed_by` (Document 18 §4.5) should be `None` for every automated
/// decision (`AutoMatchedExact`/`AutoMatchedScored`/`AmbiguousPending`/
/// `NewCanonical` — there is no human reviewer) and `Some(actor)` for
/// manual resolutions (`ManuallyConfirmed`/`ManuallyCorrected`/
/// `RejectedMatch`, Doc 30 TASK-DEDUP-007's "reviewed_by set" requirement).
pub fn append_match_decision(
    conn: &Connection,
    observation_id: &str,
    matched_canonical_id: Option<&str>,
    score: f64,
    decision: DecisionType,
    reviewed_by: Option<&str>,
) -> Result<()> {
    let id = Uuid::new_v4().to_string();
    // TASK-DB-011 fix: Document 18 §4.5's authoritative review_status enum is
    // not_required/pending_review/reviewed -- was "unreviewed"/"needs_review",
    // which never matched it. Zero other code read these exact strings
    // (checked before changing), so this is a pure internal-consistency fix.
    let review_status = if let DecisionType::AmbiguousPending(_) = decision {
        "pending_review"
    } else if reviewed_by.is_some() {
        "reviewed"
    } else {
        "not_required"
    };

    conn.execute(
        "INSERT INTO match_decisions (id, observation_id, matched_transaction_id, score, decision, review_status, reviewed_by, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, CURRENT_TIMESTAMP)",
        params![id, observation_id, matched_canonical_id, score, decision.as_str(), review_status, reviewed_by],
    )?;

    Ok(())
}

/// Creates a corrected match_decisions row when a user corrects an auto-matched transaction.
/// The original auto-match row is preserved for full traceability (Doc 11 §9.1).
pub fn append_correction_decision(
    conn: &Connection,
    observation_id: &str,
    new_canonical_id: Option<&str>,
    original_decision_id: &str,
) -> Result<()> {
    let id = Uuid::new_v4().to_string();

    conn.execute(
        "INSERT INTO match_decisions (id, observation_id, matched_transaction_id, score, decision, review_status, created_at)
         VALUES (?1, ?2, ?3, 1.0, 'manually_corrected', 'reviewed', CURRENT_TIMESTAMP)",
        params![id, observation_id, new_canonical_id],
    )?;

    // We do NOT update the original_decision_id here, to preserve immutability.

    let audit_id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO audit_log (id, actor_type, actor_id, action, resource_type, resource_id, created_at)
         VALUES (?1, 'user', 'user_id', 'manual_correction', 'match_decision', ?2, CURRENT_TIMESTAMP)",
        params![audit_id, original_decision_id],
    )?;

    Ok(())
}

/// Everything the learning worker needs about one corrected field, captured at
/// correction time.
///
/// `source_text` is copied here rather than re-fetched by the worker on
/// purpose: the retention sweep nulls `raw_payload_json` on reconciled
/// observations past a year, and a job that outlived that sweep would silently
/// stop learning. Copying costs one string per correction.
#[derive(Debug, Clone)]
pub struct CorrectionContext {
    pub feedback_log_id: String,
    pub bank_name: String,
    pub source_type: String,
    pub source_text: Option<String>,
    pub observation_id: Option<String>,
    pub field_name: String,
    pub old_value: Option<String>,
    pub new_value: String,
}

/// Writes the `feedback_log` audit row for one corrected field, decays the rule
/// that produced the wrong value, and returns the context needed to learn a
/// better one.
///
/// This function used to also *author* a rule -- `{"regex": "learned regex for
/// <new_value>"}` -- which could never match anything and quietly filled the
/// table with rules that did nothing. Authoring now belongs to
/// `learning::worker`, which is the only place that can actually validate what
/// it writes. What stays here is the half that was always correct: a correction
/// is direct evidence that whichever rule fired was wrong, so decay it.
///
/// Returns `Ok(None)` when the transaction has no observation -- a manually
/// created transaction has no source text and nothing to learn from. That is a
/// normal outcome, not an error.
pub fn log_user_correction(
    conn: &Connection,
    tx_id: &str,
    field_name: &str,
    old_value: &str,
    new_value: &str,
) -> Result<Option<CorrectionContext>> {
    let obs_info: rusqlite::Result<(
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    )> = conn.query_row(
        "SELECT o.id, o.source_pipeline, o.source_record_id, o.raw_payload_json,
                i.issuer_name, e.description_raw
         FROM transaction_observations o
         LEFT JOIN instruments i ON i.id = o.instrument_id
         LEFT JOIN statement_entries e ON e.id = o.statement_entry_id
         WHERE o.canonical_transaction_id = ?1 LIMIT 1",
        rusqlite::params![tx_id],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        },
    );

    let (obs_id, source_pipeline, source_record_id, raw_payload_json, issuer_name, row_text) =
        match obs_info {
            Ok(info) => info,
            Err(_) => return Ok(None),
        };
    let bank_name = issuer_name.unwrap_or_else(|| "Unknown".to_string());
    let feedback_id = Uuid::new_v4().to_string();

    let source_context_json = serde_json::json!({
        "source_record_id": source_record_id.clone().unwrap_or_default()
    });

    conn.execute(
        "INSERT INTO feedback_log (id, transaction_id, observation_id, source_pipeline, field_name, old_value, new_value, source_context_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            feedback_id,
            tx_id,
            obs_id,
            source_pipeline.clone().unwrap_or_else(|| "manual".to_string()),
            field_name,
            old_value,
            new_value,
            serde_json::to_string(&source_context_json).unwrap()
        ]
    )?;

    // A statement-sourced correction learns from the entry's own row text; an
    // email-sourced one from the persisted body.
    let is_statement = source_pipeline.as_deref() == Some("statement_pdf");
    let source_type = if is_statement { "statement_pdf" } else { "email" };
    let source_text = if is_statement {
        row_text
    } else {
        raw_payload_json
            .as_deref()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
            .and_then(|v| v.get("body").and_then(|b| b.as_str()).map(String::from))
    };

    // Decay whatever rule covered this exact shape: it fired and was wrong.
    if let Some(text) = source_text.as_deref() {
        let template_hash = crate::extraction::ladder::compute_template_hash(text);
        let existing: rusqlite::Result<String> = conn.query_row(
            "SELECT v.id FROM field_rule_variants v
             JOIN field_rules p ON p.id = v.field_rule_id
             WHERE v.template_hash = ?1 AND p.field_name = ?2 AND p.bank_name = ?3
               AND p.source_type = ?4 AND v.status IN ('active', 'trusted')",
            rusqlite::params![template_hash, field_name, bank_name, source_type],
            |row| row.get(0),
        );
        if let Ok(rule_id) = existing {
            let _ = crate::db::field_rules::record_failure(conn, &rule_id);
        }
    }

    Ok(Some(CorrectionContext {
        feedback_log_id: feedback_id,
        bank_name,
        source_type: source_type.to_string(),
        source_text,
        observation_id: Some(obs_id),
        field_name: field_name.to_string(),
        old_value: Some(old_value.to_string()),
        new_value: new_value.to_string(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    fn setup_db() -> Connection {
        crate::db::test_helpers::setup_test_db()
    }

    fn seed_observation(conn: &Connection, tx_id: &str, bank_name: &str, body: &str) {
        let instrument_id = format!("inst_{tx_id}");
        conn.execute(
            "INSERT INTO instruments (id, type, issuer_name, masked_identifier) VALUES (?1, 'credit_card', ?2, '1234')",
            params![instrument_id, bank_name],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO transactions (id, instrument_id, amount_minor) VALUES (?1, ?2, 0)",
            params![tx_id, instrument_id],
        )
        .unwrap();
        let raw_payload = serde_json::json!({ "body": body }).to_string();
        conn.execute(
            "INSERT INTO transaction_observations
                (id, canonical_transaction_id, source_pipeline, source_record_id, instrument_id, raw_payload_json)
             VALUES (?1, ?2, 'gmail_transaction', ?3, ?4, ?5)",
            params![
                format!("obs_{tx_id}"),
                tx_id,
                format!("record_{tx_id}"),
                instrument_id,
                raw_payload
            ],
        )
        .unwrap();
    }

    /// Two different banks' corrections must resolve to their own bank names,
    /// never collide into a shared "Unknown".
    #[test]
    fn test_log_user_correction_resolves_real_bank_name() {
        let conn = setup_db();
        seed_observation(&conn, "tx_hdfc", "HDFC Bank", "Rs 500 debited at Amazon");
        seed_observation(&conn, "tx_icici", "ICICI Bank", "INR 500 spent at Amazon");

        let hdfc = log_user_correction(&conn, "tx_hdfc", "amount", "400", "500")
            .unwrap()
            .unwrap();
        let icici = log_user_correction(&conn, "tx_icici", "amount", "400", "500")
            .unwrap()
            .unwrap();

        assert_eq!(hdfc.bank_name, "HDFC Bank");
        assert_eq!(icici.bank_name, "ICICI Bank");
    }

    /// When the observation has no resolvable instrument (e.g. a non-gmail
    /// pipeline with no instrument_id), fall back to "Unknown" same as
    /// before -- this is a fallback, not a regression.
    #[test]
    fn test_log_user_correction_falls_back_to_unknown_without_instrument() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO transactions (id, amount_minor) VALUES ('tx_no_inst', 0)",
            [],
        )
        .unwrap();
        let raw_payload = serde_json::json!({ "body": "some body" }).to_string();
        conn.execute(
            "INSERT INTO transaction_observations
                (id, canonical_transaction_id, source_pipeline, source_record_id, raw_payload_json)
             VALUES ('obs_no_inst', 'tx_no_inst', 'gmail_transaction', 'record_no_inst', ?1)",
            params![raw_payload],
        )
        .unwrap();

        let ctx = log_user_correction(&conn, "tx_no_inst", "amount", "400", "500")
            .unwrap()
            .unwrap();
        assert_eq!(ctx.bank_name, "Unknown");
    }

    /// The placeholder regex this function used to write ("learned regex for
    /// X") could never match anything. It is gone; authoring is the learning
    /// worker's job now, and this function's only rule-touching duty is to
    /// decay the rule that got it wrong.
    #[test]
    fn a_correction_no_longer_writes_a_placeholder_rule() {
        let conn = setup_db();
        seed_observation(&conn, "tx_p", "HDFC Bank", "Rs 500 debited at Amazon");
        log_user_correction(&conn, "tx_p", "merchant", "Amzon", "Amazon").unwrap();

        let rules: i64 = conn
            .query_row("SELECT COUNT(*) FROM field_rule_variants", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rules, 0, "authoring belongs to the learning worker, not to this hook");
    }

    /// A correction means whichever rule produced the wrong value was wrong.
    #[test]
    fn a_correction_decays_the_rule_that_produced_the_wrong_value() {
        let conn = setup_db();
        let body = "Rs 500 debited at Amazon";
        seed_observation(&conn, "tx_d", "HDFC Bank", body);
        let hash = crate::extraction::ladder::compute_template_hash(body);
        let now = chrono::Utc::now().naive_utc();
        let id = crate::db::field_rules::upsert_variant(
            &conn,
            &crate::db::field_rules::FieldRuleVariant {
                id: "rule_bad".to_string(),
                bank_name: "HDFC Bank".to_string(),
                field_name: "merchant".to_string(),
                source_type: "email".to_string(),
                template_hash: hash,
                rule_payload_json: serde_json::json!({"regex": "at (.+)", "capture_group": 1}),
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

        log_user_correction(&conn, "tx_d", "merchant", "Amzon", "Amazon").unwrap();

        let after = crate::db::field_rules::select_by_id(&conn, &id).unwrap().unwrap();
        assert_eq!(after.failure_count, 1, "the rule that fired and was wrong must decay");
    }

    /// The context the learning worker needs, captured at correction time --
    /// the retention sweep could null the body before the worker runs.
    #[test]
    fn a_correction_returns_the_context_needed_to_learn_from_it() {
        let conn = setup_db();
        seed_observation(&conn, "tx_c", "HDFC Bank", "Rs 500 debited at Amazon");
        let ctx = log_user_correction(&conn, "tx_c", "merchant", "Amzon", "Amazon")
            .unwrap()
            .expect("a gmail-sourced correction must produce context");

        assert_eq!(ctx.bank_name, "HDFC Bank");
        assert_eq!(ctx.source_type, "email");
        assert_eq!(ctx.field_name, "merchant");
        assert_eq!(ctx.new_value, "Amazon");
        assert_eq!(ctx.source_text.as_deref(), Some("Rs 500 debited at Amazon"));
        assert!(!ctx.feedback_log_id.is_empty());
    }

    /// feedback_log stays the single audit trail every trigger writes to.
    #[test]
    fn a_correction_still_writes_exactly_one_feedback_log_row() {
        let conn = setup_db();
        seed_observation(&conn, "tx_f", "HDFC Bank", "Rs 500 debited at Amazon");
        log_user_correction(&conn, "tx_f", "merchant", "Amzon", "Amazon").unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM feedback_log WHERE transaction_id = 'tx_f'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    /// A transaction with no observation has no source to learn from, and must
    /// not crash the save.
    #[test]
    fn a_correction_without_an_observation_is_a_no_op() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO transactions (id, amount_minor) VALUES ('tx_bare', 0)",
            [],
        )
        .unwrap();
        assert!(log_user_correction(&conn, "tx_bare", "merchant", "a", "b")
            .unwrap()
            .is_none());
    }
}
