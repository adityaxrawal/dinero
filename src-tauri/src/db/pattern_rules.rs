use anyhow::Result;
use chrono::NaiveDateTime;
use rusqlite::{params, Connection, OptionalExtension};
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

const ROW_JOIN_SELECT: &str = "SELECT v.id, p.bank_name, v.template_hash, p.field_name, \
     v.rule_payload_json, v.status, v.success_count, v.failure_count, v.confidence, \
     v.created_at, v.updated_at \
     FROM pattern_rule_variants v \
     JOIN pattern_rules p ON p.id = v.pattern_rule_id";

fn map_row(row: &rusqlite::Row) -> rusqlite::Result<PatternRulesRow> {
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
}

/// Returns the parent row's id for `(bank_name, field_name)` -- the "one
/// pattern rule per bank per category" -- creating it if this is the first
/// variant ever learned for that pair.
fn get_or_create_parent(conn: &Connection, bank_name: &str, field_name: &str) -> Result<String> {
    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM pattern_rules WHERE bank_name = ?1 AND field_name = ?2",
            params![bank_name, field_name],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(id) = existing {
        return Ok(id);
    }
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO pattern_rules (id, bank_name, field_name, created_at, updated_at)
         VALUES (?1, ?2, ?3, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        params![id, bank_name, field_name],
    )?;
    Ok(id)
}

pub fn insert(conn: &Connection, rule: &PatternRulesRow) -> Result<()> {
    // Basic state enforcement
    if !["pending", "active", "trusted", "inactive", "flagged"].contains(&rule.status.as_str()) {
        return Err(anyhow::anyhow!("Invalid status: {}", rule.status));
    }
    let parent_id = get_or_create_parent(conn, &rule.bank_name, &rule.field_name)?;
    conn.execute(
        "INSERT INTO pattern_rule_variants (
            id, pattern_rule_id, template_hash, rule_payload_json, status, success_count, failure_count, confidence, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            rule.id,
            parent_id,
            rule.template_hash,
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
        "SELECT status FROM pattern_rule_variants WHERE id = ?1",
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
        "UPDATE pattern_rule_variants SET status = ?2, updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
        params![id, new_status],
    )?;
    Ok(())
}

pub fn select_by_id(conn: &Connection, id: &str) -> Result<Option<PatternRulesRow>> {
    let sql = format!("{} WHERE v.id = ?1", ROW_JOIN_SELECT);
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query_map([id], map_row)?;
    if let Some(row) = rows.next() {
        Ok(Some(row?))
    } else {
        Ok(None)
    }
}

/// Doc 30 TASK-API-008: `settings_pattern_rules_list` -- the full ruleset
/// for the Settings management view.
pub fn select_all(conn: &Connection) -> Result<Vec<PatternRulesRow>> {
    let sql = format!("{} ORDER BY p.bank_name ASC, p.field_name ASC", ROW_JOIN_SELECT);
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], map_row)?;
    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

/// Returns the number of active or trusted variants for the given
/// `(bank_name, template_hash)` pair -- used by the drift detector to tell
/// a *known* template (rules exist but extraction failed) from a *new* one.
pub fn count_active_rules_by_bank_and_hash(
    conn: &Connection,
    bank_name: &str,
    template_hash: &str,
) -> Result<i64> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM pattern_rule_variants v
         JOIN pattern_rules p ON p.id = v.pattern_rule_id
         WHERE p.bank_name = ?1 AND v.template_hash = ?2 AND v.status IN ('active', 'trusted')",
        params![bank_name, template_hash],
        |row| row.get(0),
    )?;
    Ok(count)
}

/// Whether a still-live variant (pending/active/trusted) already exists for
/// this exact `(bank_name, template_hash, field_name)` triple. `inactive`/
/// `flagged` variants don't count -- a fresh candidate for that same
/// template is allowed to try again.
fn candidate_exists(
    conn: &Connection,
    bank_name: &str,
    template_hash: &str,
    field_name: &str,
) -> Result<bool> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM pattern_rule_variants v
            JOIN pattern_rules p ON p.id = v.pattern_rule_id
            WHERE p.bank_name = ?1 AND p.field_name = ?3 AND v.template_hash = ?2
              AND v.status IN ('pending', 'active', 'trusted')
        )",
        params![bank_name, template_hash, field_name],
        |row| row.get(0),
    )?;
    Ok(exists)
}

/// Inserts a synthesized pattern-rule candidate in `pending` state.
///
/// This is the write path for the drift detector after a successful Layer-5
/// (LLM) extraction, and for `BankTemplateLayer`'s "seed a learnable rule on
/// every match" behavior.  It enforces that only `pending` rows are written
/// through this function; callers that need to write other states must use
/// [`insert`] directly.
///
/// No-ops (returns `Ok(())` without inserting) when a candidate already
/// exists for this triple -- without this, every matching email re-inserts a
/// fresh row forever, since pending rows are never read by extraction and so
/// never earn the successes needed to leave `pending`. When it does insert,
/// the new variant is appended under the `(bank_name, field_name)` parent
/// (creating the parent if this is its first variant) -- how a bank's rule
/// set evolves as new email template formats are seen.
pub fn insert_pending_candidate(conn: &Connection, rule: &PatternRulesRow) -> Result<()> {
    if rule.status != "pending" {
        return Err(anyhow::anyhow!(
            "insert_pending_candidate requires status = 'pending', got '{}'",
            rule.status
        ));
    }
    if candidate_exists(conn, &rule.bank_name, &rule.template_hash, &rule.field_name)? {
        return Ok(());
    }
    insert(conn, rule)
}

/// Hard-deletes a pattern-rule variant. No soft-delete state -- once
/// removed, it's gone. If it was the last variant under its parent, the
/// now-empty parent row is deleted too, so an emptied-out (bank, category)
/// doesn't linger as a dangling parent.
pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    let parent_id: Option<String> = conn
        .query_row(
            "SELECT pattern_rule_id FROM pattern_rule_variants WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .optional()?;
    let deleted = conn.execute("DELETE FROM pattern_rule_variants WHERE id = ?1", params![id])?;
    if deleted == 0 {
        return Err(anyhow::anyhow!("Rule not found"));
    }
    if let Some(parent_id) = parent_id {
        conn.execute(
            "DELETE FROM pattern_rules WHERE id = ?1
             AND NOT EXISTS (SELECT 1 FROM pattern_rule_variants WHERE pattern_rule_id = ?1)",
            params![parent_id],
        )?;
    }
    Ok(())
}

/// Updates a rule's regex payload. If the rule was `active`/`trusted` (i.e.
/// actually live in extraction), editing its pattern resets it to `pending`
/// with success/failure counts zeroed -- an edited rule re-earns trust
/// rather than silently keeping the confidence its *old* regex built up.
/// A `pending`/`inactive`/`flagged` rule isn't live, so its payload updates
/// in place without disturbing status/counts.
pub fn update_payload(
    conn: &Connection,
    id: &str,
    new_payload: &serde_json::Value,
) -> Result<PatternRulesRow> {
    let existing_status: String = conn.query_row(
        "SELECT status FROM pattern_rule_variants WHERE id = ?1",
        params![id],
        |row| row.get(0),
    )?;
    let payload_str = serde_json::to_string(new_payload)?;

    if existing_status == "active" || existing_status == "trusted" {
        conn.execute(
            "UPDATE pattern_rule_variants
             SET rule_payload_json = ?2, status = 'pending',
                 success_count = 0, failure_count = 0, confidence = 0.0,
                 updated_at = CURRENT_TIMESTAMP
             WHERE id = ?1",
            params![id, payload_str],
        )?;
    } else {
        conn.execute(
            "UPDATE pattern_rule_variants SET rule_payload_json = ?2, updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
            params![id, payload_str],
        )?;
    }

    select_by_id(conn, id)?.ok_or_else(|| anyhow::anyhow!("Rule not found after update"))
}

/// Inserts a hand-written rule (Settings "Add pattern" flow). Unlike
/// [`insert_pending_candidate`], a duplicate exact `(bank, template_hash,
/// field)` triple is a hard error here -- the user explicitly asked to
/// create this rule and should be told it already exists rather than have
/// the request silently swallowed. A *new* template for a bank+field that
/// already has other variants is allowed and simply becomes another
/// variant under the same parent.
pub fn create_manual(conn: &Connection, rule: &PatternRulesRow) -> Result<PatternRulesRow> {
    if candidate_exists(conn, &rule.bank_name, &rule.template_hash, &rule.field_name)? {
        return Err(anyhow::anyhow!(
            "A pattern rule already exists for {} / {}",
            rule.bank_name,
            rule.field_name
        ));
    }
    insert(conn, rule)?;
    Ok(rule.clone())
}

/// Every active/trusted variant for a bank, across all template formats.
/// `ladder.rs`'s `LearnedPatternLayer` tries each returned rule's regex
/// against the current email body; only a genuinely matching regex
/// produces a capture, so variants learned from different templates
/// naturally coexist here without any matching-logic changes.
pub fn select_active_rules_by_bank(conn: &Connection, bank_name: &str) -> Result<Vec<PatternRulesRow>> {
    let sql = format!(
        "{} WHERE p.bank_name = ?1 AND v.status IN ('active', 'trusted')",
        ROW_JOIN_SELECT
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![bank_name], map_row)?;
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
        "UPDATE pattern_rule_variants
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
        "UPDATE pattern_rule_variants
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

    // ── Rules are scoped to bank_name (variants inside are template-scoped) ──
    /// A rule for HDFC must NOT appear when querying ICICI, even though both
    /// exist in the same DB. Verifies the self-learning system never leaks
    /// learned patterns across banks. Also verifies `select_active_rules_by_bank`
    /// (post-restructure) no longer needs -- or accepts -- a template_hash.
    #[test]
    fn test_learned_rule_scoped_to_bank() {
        let conn = setup_db();

        let hash_hdfc = "hash_hdfc_template_1";
        let hash_icici = "hash_icici_template_1";

        let now = chrono::Utc::now().naive_utc();

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

        let hdfc_results = select_active_rules_by_bank(&conn, "HDFC").unwrap();
        assert_eq!(hdfc_results.len(), 1, "should find exactly 1 HDFC rule");
        assert_eq!(hdfc_results[0].id, "hdfc_r1");
        assert_eq!(hdfc_results[0].bank_name, "HDFC");

        let icici_results = select_active_rules_by_bank(&conn, "ICICI").unwrap();
        assert_eq!(icici_results.len(), 1, "should find exactly 1 ICICI rule");
        assert_eq!(icici_results[0].id, "icici_r1");

        let hdfc_count = count_active_rules_by_bank_and_hash(&conn, "HDFC", hash_hdfc).unwrap();
        assert_eq!(hdfc_count, 1);
        let icici_cross_count =
            count_active_rules_by_bank_and_hash(&conn, "ICICI", hash_hdfc).unwrap();
        assert_eq!(icici_cross_count, 0, "count must be 0 for cross-bank query");
    }

    // ── Two templates for the same bank+field share one parent, and merge ────
    /// Core requirement: a second email template format for a (bank, field)
    /// already covered must append as a new variant under the SAME parent,
    /// not create a second parent, and must not touch the first variant.
    #[test]
    fn test_second_template_merges_under_same_parent() {
        let conn = setup_db();
        let v1 = make_rule("v1", "HDFC", "hash_old_template", "active");
        insert(&conn, &v1).unwrap();

        let v2 = make_rule("v2", "HDFC", "hash_new_template", "pending");
        insert(&conn, &v2).unwrap();

        let parent_ids: Vec<String> = conn
            .prepare("SELECT DISTINCT pattern_rule_id FROM pattern_rule_variants WHERE id IN ('v1', 'v2')")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<String>>>()
            .unwrap();
        assert_eq!(
            parent_ids.len(),
            1,
            "both variants must share exactly one parent, got {:?}",
            parent_ids
        );

        let parent_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pattern_rules WHERE bank_name = 'HDFC' AND field_name = 'amount'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(parent_count, 1, "only one parent row for HDFC/amount");

        // v1 must be untouched by v2's insert.
        let survivor = select_by_id(&conn, "v1").unwrap().unwrap();
        assert_eq!(survivor.status, "active");
        assert_eq!(survivor.rule_payload_json, v1.rule_payload_json);
    }

    // ── select_active_rules_by_bank returns variants across all templates ────
    #[test]
    fn test_select_active_rules_by_bank_spans_all_templates() {
        let conn = setup_db();
        insert(&conn, &make_rule("t1", "SBI", "hash_a", "active")).unwrap();
        insert(&conn, &make_rule("t2", "SBI", "hash_b", "trusted")).unwrap();

        let results = select_active_rules_by_bank(&conn, "SBI").unwrap();
        let ids: std::collections::HashSet<_> = results.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(results.len(), 2);
        assert!(ids.contains("t1"));
        assert!(ids.contains("t2"));
    }

    // ── deleting the last variant under a parent removes the parent too ──────
    #[test]
    fn test_delete_last_variant_removes_parent() {
        let conn = setup_db();
        let rule = make_rule("solo", "Kotak", "hash_solo", "active");
        insert(&conn, &rule).unwrap();

        delete(&conn, "solo").unwrap();

        let parent_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pattern_rules WHERE bank_name = 'Kotak' AND field_name = 'amount'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(parent_count, 0, "orphaned parent must be cleaned up");
    }

    // ── deleting one of two variants keeps the parent alive ──────────────────
    #[test]
    fn test_delete_one_of_two_variants_keeps_parent() {
        let conn = setup_db();
        insert(&conn, &make_rule("keep", "Kotak", "hash_keep", "active")).unwrap();
        insert(&conn, &make_rule("gone", "Kotak", "hash_gone", "pending")).unwrap();

        delete(&conn, "gone").unwrap();

        assert!(select_by_id(&conn, "keep").unwrap().is_some());
        let parent_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pattern_rules WHERE bank_name = 'Kotak' AND field_name = 'amount'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(parent_count, 1, "parent must survive while a sibling variant remains");
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

    // ── insert_pending_candidate is a no-op when a live candidate exists ──────
    // Root-cause coverage for the duplicate-row bug: BankTemplateLayer calls
    // this on every matching email, with no existence check previously, so
    // repeats of the same (bank, template_hash, field_name) silently piled up.
    #[test]
    fn test_insert_pending_candidate_skips_duplicate() {
        let conn = setup_db();
        let rule = make_rule("dup1", "HDFC", "hash_dup", "pending");
        insert_pending_candidate(&conn, &rule).unwrap();

        let mut second = make_rule("dup2", "HDFC", "hash_dup", "pending");
        second.rule_payload_json = serde_json::json!({"regex": r"Rs (\d+) totally different"});
        insert_pending_candidate(&conn, &second).unwrap();

        // Only the first row should exist -- the second call must be a no-op.
        assert!(
            select_by_id(&conn, "dup2").unwrap().is_none(),
            "duplicate candidate must not be inserted"
        );
        let survivor = select_by_id(&conn, "dup1").unwrap().unwrap();
        assert_eq!(survivor.rule_payload_json, rule.rule_payload_json);
    }

    // ── insert_pending_candidate allows a fresh candidate after rejection ─────
    #[test]
    fn test_insert_pending_candidate_allowed_after_inactive() {
        let conn = setup_db();
        let mut rejected = make_rule("rej1", "HDFC", "hash_rej", "pending");
        insert(&conn, &rejected).unwrap();
        update_status(&conn, "rej1", "inactive").unwrap();
        rejected.status = "inactive".to_string();

        let fresh = make_rule("rej2", "HDFC", "hash_rej", "pending");
        insert_pending_candidate(&conn, &fresh).unwrap();

        assert!(
            select_by_id(&conn, "rej2").unwrap().is_some(),
            "a fresh candidate must be allowed once the prior one is inactive"
        );
    }

    // ── delete hard-removes a rule ─────────────────────────────────────────────
    #[test]
    fn test_delete_removes_rule() {
        let conn = setup_db();
        let rule = make_rule("del1", "HDFC", "hash_del", "active");
        insert(&conn, &rule).unwrap();

        delete(&conn, "del1").unwrap();
        assert!(select_by_id(&conn, "del1").unwrap().is_none());
    }

    #[test]
    fn test_delete_missing_rule_errors() {
        let conn = setup_db();
        assert!(delete(&conn, "does-not-exist").is_err());
    }

    // ── update_payload resets active/trusted rules to pending ─────────────────
    #[test]
    fn test_update_payload_resets_active_rule_to_pending() {
        let conn = setup_db();
        let mut rule = make_rule("edit1", "HDFC", "hash_edit", "active");
        rule.success_count = 8;
        rule.confidence = 0.95;
        insert(&conn, &rule).unwrap();

        let updated =
            update_payload(&conn, "edit1", &serde_json::json!({"regex": r"New (\d+)"})).unwrap();

        assert_eq!(updated.status, "pending", "editing a live rule must reset it to pending");
        assert_eq!(updated.success_count, 0);
        assert_eq!(updated.failure_count, 0);
        assert_eq!(updated.confidence, 0.0);
        assert_eq!(updated.rule_payload_json["regex"], "New (\\d+)");
    }

    // ── update_payload edits pending rules in place ────────────────────────────
    #[test]
    fn test_update_payload_edits_pending_rule_in_place() {
        let conn = setup_db();
        let mut rule = make_rule("edit2", "HDFC", "hash_edit2", "pending");
        rule.success_count = 1;
        insert(&conn, &rule).unwrap();

        let updated =
            update_payload(&conn, "edit2", &serde_json::json!({"regex": r"New (\d+)"})).unwrap();

        assert_eq!(updated.status, "pending");
        assert_eq!(updated.success_count, 1, "non-live rule keeps its counts");
    }

    // ── create_manual rejects a duplicate triple ───────────────────────────────
    #[test]
    fn test_create_manual_rejects_duplicate() {
        let conn = setup_db();
        let rule = make_rule("man1", "Axis", "hash_man", "active");
        insert(&conn, &rule).unwrap();

        let dup = make_rule("man2", "Axis", "hash_man", "active");
        let result = create_manual(&conn, &dup);
        assert!(result.is_err(), "creating a duplicate triple must be rejected");
    }

    #[test]
    fn test_create_manual_accepts_new_triple() {
        let conn = setup_db();
        let rule = make_rule("man3", "Axis", "hash_man2", "active");
        let created = create_manual(&conn, &rule).unwrap();
        assert_eq!(created.id, "man3");
        assert!(select_by_id(&conn, "man3").unwrap().is_some());
    }
}
