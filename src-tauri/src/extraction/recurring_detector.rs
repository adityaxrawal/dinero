//! Doc 30 TASK-TXN-011: Recurring Payment Detection.
//!
//! After each canonical create/update, looks for 3+ prior occurrences of the
//! same instrument + merchant with a matching (±5%) amount at roughly
//! regular intervals, and creates/updates a `recurring_payments` row.
//! Alerting on a missed expected payment is explicitly out of scope here
//! (Area 13, notifications).

use crate::db::recurring_payments::{self, RecurringPaymentsRow};
use crate::db::transactions::find_prior_occurrences_for_merchant;
use anyhow::Result;
use chrono::{NaiveDateTime, Utc};
use rusqlite::Connection;
use uuid::Uuid;

/// Doc 30: "matching (or ±5%) amount".
const AMOUNT_TOLERANCE_FRACTION: f64 = 0.05;

/// Doc 30: "on 3+ consistent-interval occurrences" -- 3 occurrences yield 2
/// intervals, the minimum needed to say anything about regularity at all.
const MIN_OCCURRENCES: usize = 3;

fn amount_within_tolerance(a: i64, b: i64) -> bool {
    if b == 0 {
        return a == 0;
    }
    let diff = (a - b).abs() as f64;
    diff <= (b.abs() as f64) * AMOUNT_TOLERANCE_FRACTION
}

/// Doc 18 §4.14: `cadence` is `monthly` / `weekly` / `custom` -- only three
/// values, not a finer-grained frequency taxonomy. Classifies by mean
/// interval with a generous tolerance band around each anchor.
fn classify_cadence(mean_interval_days: f64) -> &'static str {
    if (5.0..=9.0).contains(&mean_interval_days) {
        "weekly"
    } else if (25.0..=35.0).contains(&mean_interval_days) {
        "monthly"
    } else {
        "custom"
    }
}

/// Doc 30: "a confidence score derived from interval-regularity standard
/// deviation" -- coefficient-of-variation based: a mean-relative stddev of 0
/// (perfectly regular) yields confidence 1.0; growing irregularity decays it
/// toward 0, floored there rather than going negative.
fn confidence_from_intervals(intervals_days: &[f64]) -> f64 {
    let n = intervals_days.len() as f64;
    let mean = intervals_days.iter().sum::<f64>() / n;
    if mean <= 0.0 {
        return 0.0;
    }
    let variance = intervals_days
        .iter()
        .map(|d| (d - mean).powi(2))
        .sum::<f64>()
        / n;
    let stddev = variance.sqrt();
    let coefficient_of_variation = stddev / mean;
    (1.0 - coefficient_of_variation).clamp(0.0, 1.0)
}

/// Runs detection for one instrument+merchant pair after a canonical
/// transaction is created or updated. `merchant_entity_id: None` (merchant
/// couldn't be resolved to a canonical entity) means there is nothing to
/// group by, so this is a no-op -- consistent with Doc 15 §2 principle 8's
/// "never guess" discipline applied to grouping, not just instrument
/// identity.
pub fn detect_and_update_recurring(
    conn: &Connection,
    instrument_id: &str,
    merchant_entity_id: Option<&str>,
    transaction_id: &str,
    amount_minor: i64,
    direction: &str,
    event_time: NaiveDateTime,
) -> Result<()> {
    // Doc 30: recurring detection is about debit subscriptions/bills;
    // credits (refunds, salary, etc.) are a different pattern entirely.
    if direction != "debit" {
        return Ok(());
    }
    let Some(merchant_entity_id) = merchant_entity_id else {
        return Ok(());
    };

    let priors = find_prior_occurrences_for_merchant(
        conn,
        instrument_id,
        merchant_entity_id,
        transaction_id,
    )?;

    let mut occurrences: Vec<(NaiveDateTime, i64)> = priors
        .iter()
        .filter(|t| {
            t.amount_minor
                .map(|a| amount_within_tolerance(a, amount_minor))
                .unwrap_or(false)
        })
        .filter_map(|t| t.best_event_time.map(|dt| (dt, t.amount_minor.unwrap())))
        .collect();
    occurrences.push((event_time, amount_minor));
    occurrences.sort_by_key(|(dt, _)| *dt);

    if occurrences.len() < MIN_OCCURRENCES {
        return Ok(());
    }

    let intervals_days: Vec<f64> = occurrences
        .windows(2)
        .map(|w| (w[1].0 - w[0].0).num_seconds() as f64 / 86400.0)
        .collect();
    let mean_interval_days = intervals_days.iter().sum::<f64>() / intervals_days.len() as f64;
    let confidence = confidence_from_intervals(&intervals_days);
    let cadence = classify_cadence(mean_interval_days);

    let last_event_time = occurrences.last().unwrap().0;
    let next_predicted_date = (last_event_time.date()
        + chrono::Duration::days(mean_interval_days.round() as i64))
    .format("%Y-%m-%d")
    .to_string();
    let next_predicted_date = chrono::NaiveDate::parse_from_str(&next_predicted_date, "%Y-%m-%d")?;

    let existing = recurring_payments::find_by_instrument_and_merchant(
        conn,
        instrument_id,
        merchant_entity_id,
    )?;

    match existing {
        Some(mut row) => {
            row.amount_minor = Some(amount_minor);
            row.cadence = Some(cadence.to_string());
            row.next_predicted_date = Some(next_predicted_date);
            row.next_predicted_amount = Some(amount_minor as f64 / 100.0);
            row.confidence = Some(confidence);
            recurring_payments::update(conn, &row)?;
        }
        None => {
            let now = Utc::now();
            let row = RecurringPaymentsRow {
                id: Uuid::new_v4().to_string(),
                merchant_entity_id: Some(merchant_entity_id.to_string()),
                instrument_id: Some(instrument_id.to_string()),
                amount_minor: Some(amount_minor),
                currency: None,
                cadence: Some(cadence.to_string()),
                next_billing_date: None,
                next_predicted_date: Some(next_predicted_date),
                next_predicted_amount: Some(amount_minor as f64 / 100.0),
                confidence: Some(confidence),
                status: Some("active".to_string()),
                source: "inferred".to_string(),
                external_mandate_id: None,
                created_at: now,
                updated_at: now,
            };
            recurring_payments::insert(conn, &row)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::transactions::{insert_transaction, TransactionsRow};

    fn setup_db() -> Connection {
        let conn = crate::db::test_helpers::setup_test_db();
        conn.execute(
            "INSERT INTO instruments (id, type, issuer_name, masked_identifier, status) VALUES ('inst_1', 'credit_card', 'HDFC', 'XXXX1234', 'active')",
            [],
        ).unwrap();
        // Doc 30 TASK-TXN-007's migration seed already ships a real
        // "Netflix"/"NETFLIX" merchant row -- a fictional name avoids the
        // UNIQUE(normalized_name) collision that pattern already caused
        // once in extraction/merchant_normalizer.rs's own tests.
        conn.execute(
            "INSERT INTO merchants (id, name, normalized_name, source) VALUES ('merch_1', 'Acme Streaming', 'ACME STREAMING', 'system')",
            [],
        ).unwrap();
        conn
    }

    fn insert_occurrence(conn: &Connection, id: &str, amount_minor: i64, date: &str) {
        let dt = NaiveDateTime::parse_from_str(&format!("{date} 10:00:00"), "%Y-%m-%d %H:%M:%S")
            .unwrap();
        insert_transaction(
            conn,
            &TransactionsRow {
                id: id.to_string(),
                unique_event_id: None,
                instrument_id: Some("inst_1".to_string()),
                instrument_type: None,
                direction: Some("debit".to_string()),
                amount: Some(amount_minor as f64 / 100.0),
                amount_minor: Some(amount_minor),
                currency: Some("INR".to_string()),
                authorization_time: Some(dt),
                best_event_time: Some(dt),
                event_time_confidence: None,
                best_posting_date: Some(dt.date()),
                posting_date_confidence: None,
                merchant_display_name: Some("Acme Streaming".to_string()),
                merchant_normalized_name: Some("ACME STREAMING".to_string()),
                merchant_entity_id: Some("merch_1".to_string()),
                reference_id: None,
                location: None,
                original_amount_minor: None,
                original_currency: None,
                exchange_rate: None,
                balance_after_transaction: None,
                status: Some("posted".to_string()),
                match_confidence: None,
                source_mix: Some("email_only".to_string()),
                alert_fired: Some(false),
                parent_transaction_id: None,
                transaction_subtype: None,
                emi_group_id: None,
                category_id: None,
                notes: None,
                is_deleted: false,
                created_at: None,
                updated_at: None,
            },
        )
        .unwrap();
    }

    /// Doc 30 TASK-TXN-011 acceptance test.
    #[test]
    fn test_recurring_detected_after_3_occurrences() {
        let conn = setup_db();
        insert_occurrence(&conn, "tx_1", 49900, "2026-03-15");
        insert_occurrence(&conn, "tx_2", 49900, "2026-04-15");
        // The 3rd occurrence is the "current" one being evaluated (not yet
        // in the transactions table when detection runs -- but the function
        // only reads OTHER rows via find_prior_occurrences_for_merchant, so
        // insert it too, matching how canonical.rs would have already
        // inserted the row before calling this function).
        insert_occurrence(&conn, "tx_3", 49900, "2026-05-15");

        let event_time =
            NaiveDateTime::parse_from_str("2026-05-15 10:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
        detect_and_update_recurring(
            &conn,
            "inst_1",
            Some("merch_1"),
            "tx_3",
            49900,
            "debit",
            event_time,
        )
        .unwrap();

        let row = recurring_payments::find_by_instrument_and_merchant(&conn, "inst_1", "merch_1")
            .unwrap()
            .expect("a recurring_payments row must have been created");
        assert_eq!(row.cadence, Some("monthly".to_string()));
        assert_eq!(row.next_predicted_amount, Some(499.0));
        assert!(
            row.confidence.unwrap() > 0.9,
            "near-perfectly-regular monthly interval should score high confidence"
        );
    }

    /// Doc 30 TASK-TXN-011 acceptance test.
    #[test]
    fn test_recurring_confidence_reflects_interval_regularity() {
        let conn = setup_db();
        // Highly irregular intervals: 10 days, then 80 days.
        insert_occurrence(&conn, "tx_1", 49900, "2026-01-01");
        insert_occurrence(&conn, "tx_2", 49900, "2026-01-11");
        insert_occurrence(&conn, "tx_3", 49900, "2026-04-01");

        let event_time =
            NaiveDateTime::parse_from_str("2026-04-01 10:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
        detect_and_update_recurring(
            &conn,
            "inst_1",
            Some("merch_1"),
            "tx_3",
            49900,
            "debit",
            event_time,
        )
        .unwrap();

        let irregular_confidence =
            recurring_payments::find_by_instrument_and_merchant(&conn, "inst_1", "merch_1")
                .unwrap()
                .unwrap()
                .confidence
                .unwrap();

        // Compare against a near-perfectly-regular series in a second, isolated DB.
        let conn2 = setup_db();
        insert_occurrence(&conn2, "tx_1", 49900, "2026-01-01");
        insert_occurrence(&conn2, "tx_2", 49900, "2026-02-01");
        insert_occurrence(&conn2, "tx_3", 49900, "2026-03-01");
        detect_and_update_recurring(
            &conn2,
            "inst_1",
            Some("merch_1"),
            "tx_3",
            49900,
            "debit",
            NaiveDateTime::parse_from_str("2026-03-01 10:00:00", "%Y-%m-%d %H:%M:%S").unwrap(),
        )
        .unwrap();
        let regular_confidence =
            recurring_payments::find_by_instrument_and_merchant(&conn2, "inst_1", "merch_1")
                .unwrap()
                .unwrap()
                .confidence
                .unwrap();

        assert!(
            regular_confidence > irregular_confidence,
            "a more regular interval series must score higher confidence: regular={regular_confidence}, irregular={irregular_confidence}"
        );
    }

    /// Doc 30 TASK-TXN-011 acceptance test.
    #[test]
    fn test_amount_tolerance_5_percent() {
        let conn = setup_db();
        insert_occurrence(&conn, "tx_1", 50000, "2026-03-15");
        // 4% higher -- within tolerance.
        insert_occurrence(&conn, "tx_2", 52000, "2026-04-15");
        insert_occurrence(&conn, "tx_3", 50000, "2026-05-15");

        let event_time =
            NaiveDateTime::parse_from_str("2026-05-15 10:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
        detect_and_update_recurring(
            &conn,
            "inst_1",
            Some("merch_1"),
            "tx_3",
            50000,
            "debit",
            event_time,
        )
        .unwrap();
        assert!(
            recurring_payments::find_by_instrument_and_merchant(&conn, "inst_1", "merch_1")
                .unwrap()
                .is_some(),
            "amounts within 5% tolerance must still be grouped as the same recurring series"
        );

        // A second, independent case: 10% higher must NOT be grouped in.
        let conn2 = setup_db();
        insert_occurrence(&conn2, "tx_1", 50000, "2026-03-15");
        insert_occurrence(&conn2, "tx_2", 55000, "2026-04-15"); // 10% higher, outside tolerance
        insert_occurrence(&conn2, "tx_3", 50000, "2026-05-15");
        detect_and_update_recurring(
            &conn2,
            "inst_1",
            Some("merch_1"),
            "tx_3",
            50000,
            "debit",
            NaiveDateTime::parse_from_str("2026-05-15 10:00:00", "%Y-%m-%d %H:%M:%S").unwrap(),
        )
        .unwrap();
        // Only 2 qualifying occurrences (tx_1, tx_3) once tx_2 is excluded by
        // tolerance -- below the 3-occurrence minimum, so no row is created.
        assert!(
            recurring_payments::find_by_instrument_and_merchant(&conn2, "inst_1", "merch_1")
                .unwrap()
                .is_none(),
            "an amount 10% outside tolerance must not count toward the 3-occurrence minimum"
        );
    }
}
