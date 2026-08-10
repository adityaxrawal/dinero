//! Recognises instalment (EMI) transactions and links them into a group.
//!
//! An instalment plan appears as many separate charges that are really one
//! purchase. Detecting the instalment number and original amount lets them be
//! grouped, so the UI can show progress through a plan rather than a series of
//! unexplained repeating charges.
//!
//! The group id is derived deterministically from the plan's own attributes, so
//! instalments ingested weeks apart still land in the same group without needing
//! to have seen each other.
use anyhow::Result;
use regex::Regex;
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::sync::OnceLock;

static EMI_INSTALLMENT_RE: OnceLock<Regex> = OnceLock::new();
static EMI_ORIGINAL_AMOUNT_RE: OnceLock<Regex> = OnceLock::new();

/// Extracts the instalment position and total, as in "3 of 12".
pub fn detect_emi_installment_numbers(body: &str) -> Option<(i32, i32)> {
    let re = EMI_INSTALLMENT_RE.get_or_init(|| {
        Regex::new(r"(?i)(?:emi|installment)\s*(?:no\.?|number)?\s*(\d+)\s*(?:of|/|out of)\s*(\d+)")
            .unwrap()
    });
    let caps = re.captures(body)?;
    let number: i32 = caps.get(1)?.as_str().parse().ok()?;
    let total: i32 = caps.get(2)?.as_str().parse().ok()?;
    if number == 0 || total == 0 || number > total {
        return None;
    }
    Some((number, total))
}

/// Extracts the plan's original purchase amount, where the message states it.
pub fn detect_emi_original_amount_minor(body: &str) -> Option<i64> {
    let re = EMI_ORIGINAL_AMOUNT_RE.get_or_init(|| {
        Regex::new(r"(?i)converted\s+to\s+emi.{0,40}?(?:rs\.?|inr|₹)\s*([\d,]+(?:\.\d+)?)").unwrap()
    });
    let caps = re.captures(body)?;
    let raw = caps.get(1)?.as_str();
    let cleaned: String = raw
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let val: f64 = cleaned.parse().ok()?;
    Some((val * 100.0).round() as i64)
}

/// Derives a stable identifier for an instalment plan.
///
/// Computed from the plan's own attributes rather than assigned, so instalments
/// ingested weeks apart land in the same group without ever having seen each
/// other. That determinism is what makes the grouping work at all.
pub fn compute_emi_group_id(
    instrument_id: &str,
    merchant_normalized: &str,
    emi_original_amount_minor: i64,
    emi_total_installments: i32,
) -> String {
    let input = format!(
        "{}|{}|{}|{}",
        instrument_id, merchant_normalized, emi_original_amount_minor, emi_total_installments
    );
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Attaches a transaction to its instalment group.
pub fn link_emi_installment(
    conn: &Connection,
    transaction_id: &str,
    instrument_id: &str,
    merchant_normalized: Option<&str>,
    emi_total_installments: Option<i32>,
    emi_original_amount_minor: Option<i64>,
) -> Result<()> {
    let (Some(merchant_normalized), Some(total), Some(original_amount)) = (
        merchant_normalized,
        emi_total_installments,
        emi_original_amount_minor,
    ) else {
        return Ok(());
    };

    let group_id = compute_emi_group_id(instrument_id, merchant_normalized, original_amount, total);

    conn.execute(
        "UPDATE transactions SET
            emi_group_id = ?2,
            transaction_subtype = 'emi_installment',
            parent_transaction_id = COALESCE(
                (SELECT id FROM transactions t2
                 WHERE t2.emi_group_id = ?2 AND t2.id != ?1 AND t2.is_deleted = 0
                 ORDER BY t2.best_event_time ASC LIMIT 1),
                parent_transaction_id
            ),
            updated_at = CURRENT_TIMESTAMP
         WHERE id = ?1",
        params![transaction_id, group_id],
    )?;

    Ok(())
}

#[derive(serde::Serialize)]
pub struct EmiGroupSummary {
    pub installments_paid: i64,
    pub total_paid_minor: i64,
    pub total_installments: Option<i64>,
    pub installments: Vec<EmiInstallmentDetail>,
}

#[derive(serde::Serialize, Debug, PartialEq)]
pub struct EmiInstallmentDetail {
    pub transaction_id: String,
    pub amount_minor: i64,
    pub event_time: Option<chrono::NaiveDateTime>,
}

/// Summarises progress through an instalment plan.
///
/// Turns a series of otherwise unexplained repeating charges into one purchase
/// with a visible schedule.
pub fn get_emi_group_summary(conn: &Connection, emi_group_id: &str) -> Result<EmiGroupSummary> {
    let (count, total): (i64, i64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(amount_minor), 0) FROM transactions \
         WHERE emi_group_id = ?1 AND is_deleted = 0",
        params![emi_group_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;

    let total_installments: Option<i64> = conn.query_row(
        "SELECT MAX(o.emi_total_installments) FROM transaction_observations o \
         JOIN transactions t ON t.id = o.canonical_transaction_id \
         WHERE t.emi_group_id = ?1 AND o.is_deleted = 0",
        params![emi_group_id],
        |r| r.get::<_, Option<i64>>(0),
    )?;

    let mut stmt = conn.prepare(
        "SELECT id, amount_minor, best_event_time FROM transactions \
         WHERE emi_group_id = ?1 AND is_deleted = 0 ORDER BY best_event_time ASC",
    )?;
    let installments = stmt
        .query_map(params![emi_group_id], |r| {
            let amount_minor: Option<i64> = r.get(1)?;
            Ok(EmiInstallmentDetail {
                transaction_id: r.get(0)?,
                amount_minor: amount_minor.unwrap_or(0),
                event_time: r.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(EmiGroupSummary {
        installments_paid: count,
        total_paid_minor: total,
        total_installments,
        installments,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_emi_installment_numbers_various_phrasings() {
        assert_eq!(
            detect_emi_installment_numbers("Your EMI installment 3 of 12 has been processed."),
            Some((3, 12))
        );
        assert_eq!(
            detect_emi_installment_numbers("EMI 5/24 debited from your card."),
            Some((5, 24))
        );
        assert_eq!(
            detect_emi_installment_numbers("This is a regular purchase, not an EMI."),
            None
        );
        assert_eq!(
            detect_emi_installment_numbers("installment 15 of 3 processed"),
            None
        );
    }

    #[test]
    fn test_detect_emi_original_amount_minor() {
        let body =
            "Your purchase of Rs 60,000.00 has been converted to EMI. Original amount: Rs 60000.00";
        assert_eq!(detect_emi_original_amount_minor(body), Some(6000000));
        assert_eq!(
            detect_emi_original_amount_minor("no emi mention here"),
            None
        );
    }

    #[test]
    fn test_compute_emi_group_id_deterministic() {
        let a = compute_emi_group_id("inst_1", "acme electronics", 6000000, 12);
        let b = compute_emi_group_id("inst_1", "acme electronics", 6000000, 12);
        assert_eq!(a, b);

        let different_amount = compute_emi_group_id("inst_1", "acme electronics", 5000000, 12);
        assert_ne!(a, different_amount);
    }

    fn setup_db() -> Connection {
        let conn = crate::db::test_helpers::setup_test_db();
        conn.execute(
            "INSERT INTO instruments (id, type, issuer_name, masked_identifier, status) VALUES ('inst_1', 'credit_card', 'HDFC', 'XXXX1234', 'active')",
            [],
        ).unwrap();
        conn
    }

    fn insert_tx(conn: &Connection, id: &str, event_time: &str, amount_minor: i64) {
        conn.execute(
            "INSERT INTO transactions (id, instrument_id, amount_minor, currency, direction, best_event_time, is_deleted) \
             VALUES (?1, 'inst_1', ?2, 'INR', 'debit', ?3, 0)",
            params![id, amount_minor, event_time],
        ).unwrap();
    }

    #[test]
    fn test_emi_group_id_shared_across_installments() {
        let conn = setup_db();
        insert_tx(&conn, "tx_1", "2026-01-15 10:00:00", 500000);
        insert_tx(&conn, "tx_2", "2026-02-15 10:00:00", 500000);

        link_emi_installment(
            &conn,
            "tx_1",
            "inst_1",
            Some("acme electronics"),
            Some(12),
            Some(6000000),
        )
        .unwrap();
        link_emi_installment(
            &conn,
            "tx_2",
            "inst_1",
            Some("acme electronics"),
            Some(12),
            Some(6000000),
        )
        .unwrap();

        let group1: String = conn
            .query_row(
                "SELECT emi_group_id FROM transactions WHERE id = 'tx_1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let group2: String = conn
            .query_row(
                "SELECT emi_group_id FROM transactions WHERE id = 'tx_2'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            group1, group2,
            "installments of the same EMI purchase must share emi_group_id"
        );
        assert!(!group1.is_empty());
    }

    #[test]
    fn test_emi_parent_transaction_linked() {
        let conn = setup_db();
        insert_tx(&conn, "tx_1", "2026-01-15 10:00:00", 500000);
        insert_tx(&conn, "tx_2", "2026-02-15 10:00:00", 500000);

        link_emi_installment(
            &conn,
            "tx_1",
            "inst_1",
            Some("acme electronics"),
            Some(12),
            Some(6000000),
        )
        .unwrap();
        link_emi_installment(
            &conn,
            "tx_2",
            "inst_1",
            Some("acme electronics"),
            Some(12),
            Some(6000000),
        )
        .unwrap();

        let parent: Option<String> = conn
            .query_row(
                "SELECT parent_transaction_id FROM transactions WHERE id = 'tx_2'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            parent,
            Some("tx_1".to_string()),
            "the later installment must link back to the earlier (origination) one"
        );

        let subtype: String = conn
            .query_row(
                "SELECT transaction_subtype FROM transactions WHERE id = 'tx_2'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(subtype, "emi_installment");
    }

    #[test]
    fn test_non_emi_transaction_has_null_emi_group() {
        let conn = setup_db();
        insert_tx(&conn, "tx_1", "2026-01-15 10:00:00", 50000);

        link_emi_installment(&conn, "tx_1", "inst_1", None, None, None).unwrap();

        let group: Option<String> = conn
            .query_row(
                "SELECT emi_group_id FROM transactions WHERE id = 'tx_1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(group, None);
    }

    #[test]
    fn test_emi_group_summary_includes_total_installments_and_installment_list() {
        let conn = setup_db();
        insert_tx(&conn, "tx_1", "2026-01-15 10:00:00", 500000);
        insert_tx(&conn, "tx_2", "2026-02-15 10:00:00", 500000);
        link_emi_installment(
            &conn,
            "tx_1",
            "inst_1",
            Some("acme electronics"),
            Some(12),
            Some(6000000),
        )
        .unwrap();
        link_emi_installment(
            &conn,
            "tx_2",
            "inst_1",
            Some("acme electronics"),
            Some(12),
            Some(6000000),
        )
        .unwrap();

        conn.execute(
            "INSERT INTO transaction_observations (id, canonical_transaction_id, source_pipeline, emi_total_installments, is_deleted) \
             VALUES ('obs_1', 'tx_2', 'gmail_transaction', 12, 0)",
            [],
        ).unwrap();

        let group_id: String = conn
            .query_row(
                "SELECT emi_group_id FROM transactions WHERE id = 'tx_1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let summary = get_emi_group_summary(&conn, &group_id).unwrap();

        assert_eq!(summary.installments_paid, 2);
        assert_eq!(summary.total_paid_minor, 1_000_000);
        assert_eq!(summary.total_installments, Some(12));
        assert_eq!(summary.installments.len(), 2);
        assert_eq!(summary.installments[0].transaction_id, "tx_1");
        assert_eq!(summary.installments[1].transaction_id, "tx_2");
        assert_eq!(summary.installments[0].amount_minor, 500000);
    }
}
