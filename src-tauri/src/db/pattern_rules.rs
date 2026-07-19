use anyhow::Result;
use chrono::NaiveDateTime;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct PatternRulesRow {
    pub id: String,
    pub bank_name: String,
    pub template_hash: String,
    pub field_name: String,
    pub rule_payload_json: serde_json::Value,
    pub status: String,
    pub success_count: i64,
    pub failure_count: i64,
    pub confidence: f64,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
}

pub fn insert(conn: &Connection, rule: &PatternRulesRow) -> Result<()> {
    // Basic state enforcement
    if !["pending", "active", "trusted", "inactive", "flagged"].contains(&rule.status.as_str()) {
        return Err(anyhow::anyhow!("Invalid status: {}", rule.status));
    }
    conn.execute(
        "INSERT INTO pattern_rules (
            id, bank_name, template_hash, field_name, rule_payload_json, status, success_count, failure_count, confidence, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            rule.id,
            rule.bank_name,
            rule.template_hash,
            rule.field_name,
            serde_json::to_string(&rule.rule_payload_json)?,
            rule.status,
            rule.success_count,
            rule.failure_count,
            rule.confidence,
            rule.created_at,
            rule.updated_at,
        ],
    )?;
    Ok(())
}

pub fn update_status(conn: &Connection, id: &str, new_status: &str) -> Result<()> {
    if !["pending", "active", "trusted", "inactive", "flagged"].contains(&new_status) {
        return Err(anyhow::anyhow!("Invalid status: {}", new_status));
    }

    let existing_status: String = conn.query_row(
        "SELECT status FROM pattern_rules WHERE id = ?1",
        params![id],
        |row| row.get(0),
    )?;

    // Allowed transitions logic:
    // Usually pending -> active -> trusted -> inactive
    // We'll enforce some basic sanity rules here based on the requirement.
    match existing_status.as_str() {
        "pending" => {
            if new_status != "active" && new_status != "flagged" && new_status != "inactive" {
                return Err(anyhow::anyhow!(
                    "Invalid transition from pending to {}",
                    new_status
                ));
            }
        }
        "active" => {
            if new_status != "trusted" && new_status != "flagged" && new_status != "inactive" {
                return Err(anyhow::anyhow!(
                    "Invalid transition from active to {}",
                    new_status
                ));
            }
        }
        "trusted" if new_status != "inactive" && new_status != "flagged" => {
            return Err(anyhow::anyhow!(
                "Invalid transition from trusted to {}",
                new_status
            ));
        }
        _ => {}
    }

    conn.execute(
        "UPDATE pattern_rules SET status = ?2, updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
        params![id, new_status],
    )?;
    Ok(())
}

pub fn select_by_id(conn: &Connection, id: &str) -> Result<Option<PatternRulesRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, bank_name, template_hash, field_name, rule_payload_json, status, success_count, failure_count, confidence, created_at, updated_at
         FROM pattern_rules
         WHERE id = ?1"
    )?;

    let mut rows = stmt.query_map([id], |row| {
        let payload_str: String = row.get(4)?;
        let payload = serde_json::from_str(&payload_str).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?;

        Ok(PatternRulesRow {
            id: row.get(0)?,
            bank_name: row.get(1)?,
            template_hash: row.get(2)?,
            field_name: row.get(3)?,
            rule_payload_json: payload,
            status: row.get(5)?,
            success_count: row.get(6)?,
            failure_count: row.get(7)?,
            confidence: row.get(8)?,
            created_at: row.get(9)?,
            updated_at: row.get(10)?,
        })
    })?;

    if let Some(row) = rows.next() {
        Ok(Some(row?))
    } else {
        Ok(None)
    }
}

/// Doc 30 TASK-API-008: `settings_pattern_rules_list` -- the full ruleset
/// for the Settings management view. Did not exist before this task (only
/// single-row `select_by_id` and the bank/hash-scoped lookups used by
/// extraction did).
pub fn select_all(conn: &Connection) -> Result<Vec<PatternRulesRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, bank_name, template_hash, field_name, rule_payload_json, status, success_count, failure_count, confidence, created_at, updated_at
         FROM pattern_rules
         ORDER BY bank_name ASC, field_name ASC"
    )?;

    let rows = stmt.query_map([], |row| {
        let payload_str: String = row.get(4)?;
        let payload = serde_json::from_str(&payload_str).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?;

        Ok(PatternRulesRow {
            id: row.get(0)?,
            bank_name: row.get(1)?,
            template_hash: row.get(2)?,
            field_name: row.get(3)?,
            rule_payload_json: payload,
            status: row.get(5)?,
            success_count: row.get(6)?,
            failure_count: row.get(7)?,
            confidence: row.get(8)?,
            created_at: row.get(9)?,
            updated_at: row.get(10)?,
        })
    })?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

/// Returns the number of active or trusted rules for the given
/// `(bank_name, template_hash)` pair.  Used by the drift detector to decide
/// whether a given template is *known* (rules exist but extraction failed) or
/// *new* (never seen before, not a drift scenario).
pub fn count_active_rules_by_bank_and_hash(
    conn: &Connection,
    bank_name: &str,
    template_hash: &str,
) -> Result<i64> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM pattern_rules \
         WHERE bank_name = ?1 AND template_hash = ?2 AND status IN ('active', 'trusted')",
        params![bank_name, template_hash],
        |row| row.get(0),
    )?;
    Ok(count)
}

/// Inserts a synthesized pattern-rule candidate in `pending` state.
///
/// This is the write path for the drift detector after a successful Layer-5
/// (LLM) extraction.  It enforces that only `pending` rows are written through
/// this function; callers that need to write other states must use [`insert`]
/// directly.
pub fn insert_pending_candidate(conn: &Connection, rule: &PatternRulesRow) -> Result<()> {
    if rule.status != "pending" {
        return Err(anyhow::anyhow!(
            "insert_pending_candidate requires status = 'pending', got '{}'",
            rule.status
        ));
    }
    insert(conn, rule)
}

pub fn select_active_rules_by_bank_and_hash(
    conn: &Connection,
    bank_name: &str,
    template_hash: &str,
) -> Result<Vec<PatternRulesRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, bank_name, template_hash, field_name, rule_payload_json, status, success_count, failure_count, confidence, created_at, updated_at
         FROM pattern_rules
         WHERE bank_name = ?1 AND template_hash = ?2 AND status IN ('active', 'trusted')"
    )?;

    let rows = stmt.query_map(params![bank_name, template_hash], |row| {
        let payload_str: String = row.get(4)?;
        let payload = serde_json::from_str(&payload_str).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?;

        Ok(PatternRulesRow {
            id: row.get(0)?,
            bank_name: row.get(1)?,
            template_hash: row.get(2)?,
            field_name: row.get(3)?,
            rule_payload_json: payload,
            status: row.get(5)?,
            success_count: row.get(6)?,
            failure_count: row.get(7)?,
            confidence: row.get(8)?,
            created_at: row.get(9)?,
            updated_at: row.get(10)?,
        })
    })?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// Doc 30 TASK-TXN-002: a single atomic `UPDATE` -- SQLite evaluates every
/// column expression in an `UPDATE` against the pre-update row, so
/// `success_count`/`confidence`/`status` all derive from the same
/// consistent snapshot within one statement. Replaces a prior
/// `SELECT` -> mutate-in-Rust -> `UPDATE` (absolute value) round trip, which
/// lost updates when the Transaction Queue's concurrent workers both read
/// the same starting count for the same rule before either wrote back.
pub fn record_rule_success(conn: &Connection, id: &str) -> Result<()> {
    let updated = conn.execute(
        "UPDATE pattern_rules
         SET success_count = success_count + 1,
             confidence = CAST(success_count + 1 AS REAL) / (success_count + 1 + failure_count),
             status = CASE
                 WHEN status = 'pending' AND success_count + 1 >= 3 THEN 'active'
                 WHEN status = 'active' AND success_count + 1 >= 10 THEN 'trusted'
                 ELSE status
             END,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?1",
        params![id],
    )?;
    if updated == 0 {
        return Err(anyhow::anyhow!("Rule not found"));
    }
    Ok(())
}

/// Doc 30 TASK-TXN-002: see [`record_rule_success`] -- same atomic-`UPDATE`
/// fix applied to the failure/decay path.
pub fn record_rule_failure(conn: &Connection, id: &str) -> Result<()> {
    let updated = conn.execute(
        "UPDATE pattern_rules
         SET failure_count = failure_count + 1,
             confidence = CAST(success_count AS REAL) / (success_count + failure_count + 1),
             status = CASE
                 WHEN failure_count + 1 >= 3 THEN 'inactive'
                 WHEN CAST(success_count AS REAL) / (success_count + failure_count + 1) < 0.70 THEN 'inactive'
                 ELSE status
             END,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?1",
        params![id],
    )?;
    if updated == 0 {
        return Err(anyhow::anyhow!("Rule not found"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_db() -> Connection {
        crate::db::test_helpers::setup_test_db()
    }

    fn make_rule(id: &str, bank_name: &str, template_hash: &str, status: &str) -> PatternRulesRow {
        let now = chrono::Utc::now().naive_utc();
        PatternRulesRow {
            id: id.to_string(),
            bank_name: bank_name.to_string(),
            template_hash: template_hash.to_string(),
            field_name: "amount".to_string(),
            rule_payload_json: serde_json::json!({"regex": r"Rs (\d+)"}),
            status: status.to_string(),
            success_count: 0,
            failure_count: 0,
            confidence: 1.0,
            created_at: Some(now),
            updated_at: Some(now),
        }
    }

    // ── State machine: pending → active after 3 successes ─────────────────────
    #[test]
    fn test_pending_to_active_after_3_successes() {
        let conn = setup_db();
        let rule = make_rule("r1", "HDFC", "hash_a", "pending");
        insert(&conn, &rule).unwrap();

        // 1st and 2nd success → still pending
        record_rule_success(&conn, "r1").unwrap();
        record_rule_success(&conn, "r1").unwrap();
        let r = select_by_id(&conn, "r1").unwrap().unwrap();
        assert_eq!(
            r.status, "pending",
            "should still be pending after 2 successes"
        );
        assert_eq!(r.success_count, 2);

        // 3rd success → promote to active
        record_rule_success(&conn, "r1").unwrap();
        let r = select_by_id(&conn, "r1").unwrap().unwrap();
        assert_eq!(r.status, "active", "should be active after 3 successes");
        assert_eq!(r.success_count, 3);
        assert!(
            r.confidence > 0.99,
            "confidence should be 1.0 with no failures"
        );
    }

    // ── State machine: active → trusted after 10 cumulative successes ──────────
    #[test]
    fn test_active_to_trusted_after_10_successes() {
        let conn = setup_db();
        // Start at active with 3 successes already
        let mut rule = make_rule("r2", "HDFC", "hash_b", "active");
        rule.success_count = 3;
        insert(&conn, &rule).unwrap();

        // Drive from 3 → 10
        for _ in 0..7 {
            record_rule_success(&conn, "r2").unwrap();
        }
        let r = select_by_id(&conn, "r2").unwrap().unwrap();
        assert_eq!(
            r.status, "trusted",
            "should be trusted after 10 total successes"
        );
        assert_eq!(r.success_count, 10);
    }

    // ── State machine: 3 failures → inactive ──────────────────────────────────
    #[test]
    fn test_3_failures_causes_inactive() {
        let conn = setup_db();
        // Use an active rule with 5 prior successes so confidence doesn't trigger
        // decay first (5/(5+2) ≈ 0.71 > 0.70, but 3 failures triggers directly)
        let mut rule = make_rule("r3", "HDFC", "hash_c", "active");
        rule.success_count = 5;
        insert(&conn, &rule).unwrap();

        record_rule_failure(&conn, "r3").unwrap();
        record_rule_failure(&conn, "r3").unwrap();
        let r = select_by_id(&conn, "r3").unwrap().unwrap();
        assert_ne!(
            r.status, "inactive",
            "should not yet be inactive after 2 failures"
        );

        record_rule_failure(&conn, "r3").unwrap();
        let r = select_by_id(&conn, "r3").unwrap().unwrap();
        assert_eq!(r.status, "inactive", "should be inactive after 3 failures");
    }

    // ── Confidence decay: drops below 70% → inactive even before 3 failures ───
    #[test]
    fn test_confidence_decay_below_70_causes_inactive() {
        let conn = setup_db();
        // 1 success, then 3 failures → confidence = 1/4 = 25%, inactive via decay
        let mut rule = make_rule("r4", "SBI", "hash_d", "active");
        rule.success_count = 1;
        insert(&conn, &rule).unwrap();

        // 2nd failure: 1/(1+2) ≈ 0.33 < 0.70 → inactive via decay path
        record_rule_failure(&conn, "r4").unwrap();
        record_rule_failure(&conn, "r4").unwrap();
        let r = select_by_id(&conn, "r4").unwrap().unwrap();
        assert_eq!(
            r.status, "inactive",
            "confidence < 70% should mark rule inactive; got confidence={:.2}",
            r.confidence
        );
    }

    // ── Rules are scoped to (bank_name, template_hash) ────────────────────────
    /// A rule for HDFC with hash_e must NOT appear when querying ICICI or a
    /// different hash, even if both exist in the same DB.  This verifies that
    /// the self-learning system never leaks learned patterns across banks.
    #[test]
    fn test_learned_rule_scoped_to_bank_and_template() {
        let conn = setup_db();

        let hash_hdfc = "hash_hdfc_template_1";
        let hash_icici = "hash_icici_template_1";

        let now = chrono::Utc::now().naive_utc();

        // Insert an active HDFC rule
        let hdfc_rule = PatternRulesRow {
            id: "hdfc_r1".to_string(),
            bank_name: "HDFC".to_string(),
            template_hash: hash_hdfc.to_string(),
            field_name: "amount".to_string(),
            rule_payload_json: serde_json::json!({"regex": r"Rs (\d+)"}),
            status: "active".to_string(),
            success_count: 5,
            failure_count: 0,
            confidence: 1.0,
            created_at: Some(now),
            updated_at: Some(now),
        };
        insert(&conn, &hdfc_rule).unwrap();

        // Insert an active ICICI rule with a different hash
        let icici_rule = PatternRulesRow {
            id: "icici_r1".to_string(),
            bank_name: "ICICI".to_string(),
            template_hash: hash_icici.to_string(),
            field_name: "amount".to_string(),
            rule_payload_json: serde_json::json!({"regex": r"INR (\d+)"}),
            status: "active".to_string(),
            success_count: 3,
            failure_count: 0,
            confidence: 1.0,
            created_at: Some(now),
            updated_at: Some(now),
        };
        insert(&conn, &icici_rule).unwrap();

        // Querying HDFC/hash_hdfc must return exactly the HDFC rule
        let hdfc_results = select_active_rules_by_bank_and_hash(&conn, "HDFC", hash_hdfc).unwrap();
        assert_eq!(hdfc_results.len(), 1, "should find exactly 1 HDFC rule");
        assert_eq!(hdfc_results[0].id, "hdfc_r1");
        assert_eq!(hdfc_results[0].bank_name, "HDFC");

        // Querying ICICI/hash_icici must return exactly the ICICI rule
        let icici_results =
            select_active_rules_by_bank_and_hash(&conn, "ICICI", hash_icici).unwrap();
        assert_eq!(icici_results.len(), 1, "should find exactly 1 ICICI rule");
        assert_eq!(icici_results[0].id, "icici_r1");

        // Cross-query: HDFC hash in ICICI must return nothing
        let cross_results =
            select_active_rules_by_bank_and_hash(&conn, "ICICI", hash_hdfc).unwrap();
        assert!(cross_results.is_empty(), "ICICI must not see HDFC rules");

        // Count-based helper must also be scoped
        let hdfc_count = count_active_rules_by_bank_and_hash(&conn, "HDFC", hash_hdfc).unwrap();
        assert_eq!(hdfc_count, 1);
        let icici_cross_count =
            count_active_rules_by_bank_and_hash(&conn, "ICICI", hash_hdfc).unwrap();
        assert_eq!(icici_cross_count, 0, "count must be 0 for cross-bank query");
    }

    // ── insert_pending_candidate rejects non-pending status ───────────────────
    #[test]
    fn test_insert_pending_candidate_rejects_non_pending() {
        let conn = setup_db();
        let rule = make_rule("r5", "Axis", "hash_f", "active"); // wrong status
        let result = insert_pending_candidate(&conn, &rule);
        assert!(
            result.is_err(),
            "insert_pending_candidate must reject non-pending status"
        );
        assert!(
            result.unwrap_err().to_string().contains("pending"),
            "error message should mention 'pending'"
        );
    }

    // ── insert_pending_candidate accepts pending status ────────────────────────
    #[test]
    fn test_insert_pending_candidate_accepts_pending() {
        let conn = setup_db();
        let rule = make_rule("r6", "Axis", "hash_g", "pending");
        insert_pending_candidate(&conn, &rule).unwrap();
        let fetched = select_by_id(&conn, "r6").unwrap().unwrap();
        assert_eq!(fetched.status, "pending");
        assert_eq!(fetched.bank_name, "Axis");
    }

    // ── update_status enforces allowed transitions ─────────────────────────────
    #[test]
    fn test_update_status_invalid_transition_rejected() {
        let conn = setup_db();
        let rule = make_rule("r7", "HDFC", "hash_h", "pending");
        insert(&conn, &rule).unwrap();

        // pending → trusted is invalid (must go through active first)
        let result = update_status(&conn, "r7", "trusted");
        assert!(result.is_err(), "pending → trusted should be rejected");

        // pending → active is valid
        update_status(&conn, "r7", "active").unwrap();
        let r = select_by_id(&conn, "r7").unwrap().unwrap();
        assert_eq!(r.status, "active");
    }
}
