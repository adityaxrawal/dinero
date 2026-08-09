//! The statement-PDF half of the learned-rule read path (design 2026-07-29).
//!
//! Separate from `ladder::apply_learned_fields` because the two extractors are
//! structurally different, not because the rules are: an email rule runs once
//! against one body, a statement rule runs against each row's own text. The
//! rule *storage*, synthesis, validation and lifecycle are shared verbatim —
//! only the loop differs, which is why this file is small.
//!
//! The unit of source text is the row description, which is also what
//! `statement_entries.description_raw` persists and therefore what a correction
//! on a statement-sourced transaction learns from. Same text in, same text out,
//! so a rule taught by a correction fires on the next statement's matching row.

use crate::statements::row_extractor::{parse_amount_minor, parse_date, StatementRow};
use deadpool_sqlite::Pool;

/// Overlays this bank's learned statement rules onto freshly extracted rows.
///
/// Returns how many rows a rule actually changed — logged by the caller, and
/// the only signal that the learning loop is doing anything on the PDF side.
pub async fn apply_learned_rules_to_rows(
    pool: &Pool,
    bank_name: &str,
    rows: &mut [StatementRow],
) -> usize {
    if rows.is_empty() {
        return 0;
    }
    let bank = bank_name.to_string();
    let Ok(conn) = pool.get().await else {
        return 0;
    };
    let rules = match conn
        .interact(move |c| crate::db::field_rules::select_live_by_bank(c, &bank, "statement_pdf"))
        .await
    {
        Ok(Ok(r)) => r,
        _ => return 0,
    };
    if rules.is_empty() {
        return 0;
    }

    let mut changed = 0usize;
    for row in rows.iter_mut() {
        // `merchant_raw` is the row's full description at this stage (see
        // `map_rows_to_statement_entries`, which writes it to both
        // `description_raw` and `merchant_raw`), so it is the source text a
        // correction on this row would later learn from.
        let source = row.merchant_raw.clone();
        let source_hash = crate::extraction::ladder::compute_template_hash(&source);
        let mut row_changed = false;

        for rule in &rules {
            let is_override = rule.rule_payload_json.get("override_value").is_some();
            if is_override && rule.template_hash != source_hash {
                continue;
            }
            let Some(captured) =
                crate::extraction::rule_synthesis::apply_payload(&rule.rule_payload_json, &source)
            else {
                continue;
            };
            let captured = captured.trim();
            if captured.is_empty() {
                continue;
            }

            // Every arm leaves the row untouched when the capture will not
            // parse: a rule that fires but yields nonsense must degrade to a
            // no-op, never to a zeroed amount or a cleared date.
            match rule.field_name.as_str() {
                "merchant" => {
                    row.merchant_raw = captured.to_string();
                    row_changed = true;
                }
                "amount" => {
                    if let Some(v) = parse_amount_minor(captured) {
                        row.amount_minor = v;
                        row_changed = true;
                    }
                }
                "event_time" => {
                    if let Some(d) = parse_date(captured) {
                        row.transaction_date = d;
                        row_changed = true;
                    }
                }
                "reference_id" => {
                    row.reference_id = Some(captured.to_string());
                    row_changed = true;
                }
                "direction" => {
                    let d = captured.to_lowercase();
                    if d == "debit" || d == "credit" {
                        row.direction = d;
                        row_changed = true;
                    }
                }
                "currency" => {
                    row.currency = captured.to_uppercase();
                    row_changed = true;
                }
                _ => {}
            }
        }
        if row_changed {
            changed += 1;
        }
    }

    if changed > 0 {
        tracing::info!(
            bank = bank_name,
            changed,
            "applied learned statement rules to extracted rows"
        );
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::statements::row_extractor::StatementRow;

    async fn setup_pool() -> Pool {
        let path = crate::db::test_helpers::fresh_temp_db_path();
        crate::db::migrations::run_migrations(&path, None)
            .await
            .unwrap();
        let mgr = deadpool_sqlite::Manager::from_config(
            &deadpool_sqlite::Config {
                path,
                pool: Some(deadpool_sqlite::PoolConfig::new(2)),
            },
            deadpool_sqlite::Runtime::Tokio1,
        );
        Pool::builder(mgr).build().unwrap()
    }

    fn row(description: &str) -> StatementRow {
        StatementRow {
            transaction_date: "2026-07-01".to_string(),
            merchant_raw: description.to_string(),
            amount_minor: 50000,
            currency: "INR".to_string(),
            direction: "debit".to_string(),
            reference_id: None,
            row_index: 0,
            llm_extracted: false,
        }
    }

    async fn seed(pool: &Pool, field: &str, source: &str, payload: serde_json::Value) {
        let conn = pool.get().await.unwrap();
        let (f, s) = (field.to_string(), source.to_string());
        conn.interact(move |c| {
            let now = chrono::Utc::now().naive_utc();
            crate::db::field_rules::upsert_variant(
                c,
                &crate::db::field_rules::FieldRuleVariant {
                    id: uuid::Uuid::new_v4().to_string(),
                    bank_name: "HDFC".to_string(),
                    field_name: f,
                    source_type: "statement_pdf".to_string(),
                    template_hash: crate::extraction::ladder::compute_template_hash(&s),
                    rule_payload_json: payload,
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
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn a_learned_merchant_rule_rewrites_a_statement_row() {
        let pool = setup_pool().await;
        let desc = "POS 4412 RAZ*BLUE TOKAI COFFEE MUMBAI IN";
        seed(
            &pool,
            "merchant",
            desc,
            serde_json::json!({"regex": r"(?i)RAZ\*(.{1,80}?)\s+MUMBAI", "capture_group": 1}),
        )
        .await;

        let mut rows = vec![row(desc)];
        let changed = apply_learned_rules_to_rows(&pool, "HDFC", &mut rows).await;

        assert_eq!(changed, 1);
        assert_eq!(rows[0].merchant_raw, "BLUE TOKAI COFFEE");
    }

    #[tokio::test]
    async fn rows_the_rule_does_not_match_are_untouched() {
        let pool = setup_pool().await;
        seed(
            &pool,
            "merchant",
            "POS 4412 RAZ*BLUE TOKAI COFFEE MUMBAI IN",
            serde_json::json!({"regex": r"(?i)RAZ\*(.{1,80}?)\s+MUMBAI", "capture_group": 1}),
        )
        .await;

        let mut rows = vec![row("UPI-ZOMATO-9988776655-PAYMENT")];
        let changed = apply_learned_rules_to_rows(&pool, "HDFC", &mut rows).await;

        assert_eq!(changed, 0);
        assert_eq!(rows[0].merchant_raw, "UPI-ZOMATO-9988776655-PAYMENT");
    }

    /// An email-learned rule must never reach PDF extraction.
    #[tokio::test]
    async fn an_email_rule_does_not_apply_to_statement_rows() {
        let pool = setup_pool().await;
        let desc = "POS 4412 RAZ*BLUE TOKAI COFFEE MUMBAI IN";
        {
            let conn = pool.get().await.unwrap();
            let d = desc.to_string();
            conn.interact(move |c| {
                let now = chrono::Utc::now().naive_utc();
                crate::db::field_rules::upsert_variant(
                    c,
                    &crate::db::field_rules::FieldRuleVariant {
                        id: "email_rule".to_string(),
                        bank_name: "HDFC".to_string(),
                        field_name: "merchant".to_string(),
                        source_type: "email".to_string(),
                        template_hash: crate::extraction::ladder::compute_template_hash(&d),
                        rule_payload_json: serde_json::json!({
                            "regex": r"(?i)RAZ\*(.{1,80}?)\s+MUMBAI", "capture_group": 1
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
            })
            .await
            .unwrap();
        }

        let mut rows = vec![row(desc)];
        assert_eq!(
            apply_learned_rules_to_rows(&pool, "HDFC", &mut rows).await,
            0
        );
    }

    #[tokio::test]
    async fn a_learned_amount_rule_rewrites_minor_units() {
        let pool = setup_pool().await;
        let desc = "NEFT CHARGES 1,020.00 DR";
        seed(
            &pool,
            "amount",
            desc,
            serde_json::json!({"regex": r"([\d,]+\.\d{2})\s+DR", "capture_group": 1}),
        )
        .await;

        let mut rows = vec![row(desc)];
        apply_learned_rules_to_rows(&pool, "HDFC", &mut rows).await;
        assert_eq!(rows[0].amount_minor, 102000);
    }

    #[tokio::test]
    async fn a_bank_with_no_rules_costs_one_query_and_changes_nothing() {
        let pool = setup_pool().await;
        let mut rows = vec![row("ANY DESCRIPTION")];
        assert_eq!(
            apply_learned_rules_to_rows(&pool, "Kotak", &mut rows).await,
            0
        );
        assert_eq!(rows[0].merchant_raw, "ANY DESCRIPTION");
    }

    /// A capture that cannot be parsed must leave the row's value alone rather
    /// than zeroing it — a broken rule should degrade to no-op, not to data loss.
    #[tokio::test]
    async fn an_unparseable_capture_leaves_the_row_intact() {
        let pool = setup_pool().await;
        let desc = "NEFT CHARGES ABC DR";
        seed(
            &pool,
            "amount",
            desc,
            serde_json::json!({"regex": r"CHARGES\s+(\w+)\s+DR", "capture_group": 1}),
        )
        .await;

        let mut rows = vec![row(desc)];
        apply_learned_rules_to_rows(&pool, "HDFC", &mut rows).await;
        assert_eq!(
            rows[0].amount_minor, 50000,
            "the original amount must survive"
        );
    }
}
