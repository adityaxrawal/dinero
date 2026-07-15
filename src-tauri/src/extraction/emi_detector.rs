//! Doc 30 TASK-TXN-012: EMI Group Detection and Linking.
//!
//! Two halves: (1) detecting EMI language during extraction (Layers 2/3) and
//! populating the observation's EMI fields; (2) on canonical creation/
//! update, generating/reusing a shared `emi_group_id` and linking the
//! parent (origination) transaction.

use anyhow::Result;
use regex::Regex;
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::sync::OnceLock;

static EMI_INSTALLMENT_RE: OnceLock<Regex> = OnceLock::new();
static EMI_ORIGINAL_AMOUNT_RE: OnceLock<Regex> = OnceLock::new();

/// Doc 30: "detect EMI language ('EMI', 'installment X of Y', 'converted to
/// EMI')". Returns `(installment_number, total_installments)` when an
/// explicit "X of Y" pattern is found — without both numbers there is
/// nothing meaningful to populate on `emi_installment_number`/
/// `emi_total_installments`, so a bare "EMI" or "converted to EMI" mention
/// with no numbers is deliberately not enough on its own.
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

/// Doc 30: "converted to EMI" often states the pre-conversion purchase
/// amount separately from this installment's amount -- best-effort, may
/// legitimately find nothing (the amount just isn't always restated).
pub fn detect_emi_original_amount_minor(body: &str) -> Option<i64> {
    let re = EMI_ORIGINAL_AMOUNT_RE.get_or_init(|| {
        Regex::new(r"(?i)converted\s+to\s+emi.{0,40}?(?:rs\.?|inr|₹)\s*([\d,]+(?:\.\d+)?)").unwrap()
    });
    let caps = re.captures(body)?;
    let raw = caps.get(1)?.as_str();
    let cleaned: String = raw.chars().filter(|c| c.is_ascii_digit() || *c == '.').collect();
    let val: f64 = cleaned.parse().ok()?;
    Some((val * 100.0).round() as i64)
}

/// Doc 18 §4.2's exact, deterministic formula: `SHA-256(instrument_id || "|"
/// || merchant_normalized || "|" || emi_original_amount_minor || "|" ||
/// emi_total_installments)`. Deliberately not Doc 30's own looser paraphrase
/// ("matched by instrument + emi_original_amount_minor + merchant-
/// similarity") -- Document 18 is schema-authoritative (Doc 49 §6) and gives
/// an exact formula needing no fuzzy matching at all: identical inputs
/// always hash identically, so "similarity" isn't needed.
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

/// Doc 30 TASK-TXN-012: "On canonical creation/update, if EMI fields are
/// populated, generate/reuse a shared `emi_group_id`... and set
/// `transaction_subtype = 'emi_installment'`; link the parent purchase
/// transaction (if separately captured) via `parent_transaction_id`."
/// No-op if any of the three EMI inputs is missing -- nothing to group by.
pub fn link_emi_installment(
    conn: &Connection,
    transaction_id: &str,
    instrument_id: &str,
    merchant_normalized: Option<&str>,
    emi_total_installments: Option<i32>,
    emi_original_amount_minor: Option<i64>,
) -> Result<()> {
    let (Some(merchant_normalized), Some(total), Some(original_amount)) =
        (merchant_normalized, emi_total_installments, emi_original_amount_minor)
    else {
        return Ok(());
    };

    let group_id = compute_emi_group_id(instrument_id, merchant_normalized, original_amount, total);

    // The earliest other member of this group, if any, is the parent
    // (origination) transaction.
    let parent_id: Option<String> = conn
        .query_row(
            "SELECT id FROM transactions \
             WHERE emi_group_id = ?1 AND id != ?2 AND is_deleted = 0 \
             ORDER BY best_event_time ASC LIMIT 1",
            params![group_id, transaction_id],
            |r| r.get(0),
        )
        .optional()?;

    conn.execute(
        "UPDATE transactions SET
            emi_group_id = ?2,
            transaction_subtype = 'emi_installment',
            parent_transaction_id = COALESCE(?3, parent_transaction_id),
            updated_at = CURRENT_TIMESTAMP
         WHERE id = ?1",
        params![transaction_id, group_id, parent_id],
    )?;

    Ok(())
}

/// Doc 30: "Exposes an aggregate query (consumed by Area 8) for total
/// paid/remaining per EMI group." `total_expected` is left `None` -- neither
/// `transactions` nor `transaction_observations` stores
/// `emi_total_installments` on the canonical row (Document 18 §4.2 has no
/// such column; it's a hash input only), so "remaining" can only be
/// expressed as "count so far", not "count so far vs. total N" without
/// re-deriving N from the original observation. Flagged rather than guessed.
#[derive(serde::Serialize)]
pub struct EmiGroupSummary {
    pub installments_paid: i64,
    pub total_paid_minor: i64,
}

pub fn get_emi_group_summary(conn: &Connection, emi_group_id: &str) -> Result<EmiGroupSummary> {
    let (count, total): (i64, i64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(amount_minor), 0) FROM transactions \
         WHERE emi_group_id = ?1 AND is_deleted = 0",
        params![emi_group_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok(EmiGroupSummary {
        installments_paid: count,
        total_paid_minor: total,
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
        // Nonsensical (number > total) must not be accepted.
        assert_eq!(
            detect_emi_installment_numbers("installment 15 of 3 processed"),
            None
        );
    }

    #[test]
    fn test_detect_emi_original_amount_minor() {
        let body = "Your purchase of Rs 60,000.00 has been converted to EMI. Original amount: Rs 60000.00";
        assert_eq!(detect_emi_original_amount_minor(body), Some(6000000));
        assert_eq!(detect_emi_original_amount_minor("no emi mention here"), None);
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

    /// Doc 30 TASK-TXN-012 acceptance test.
    #[test]
    fn test_emi_group_id_shared_across_installments() {
        let conn = setup_db();
        insert_tx(&conn, "tx_1", "2026-01-15 10:00:00", 500000);
        insert_tx(&conn, "tx_2", "2026-02-15 10:00:00", 500000);

        link_emi_installment(&conn, "tx_1", "inst_1", Some("acme electronics"), Some(12), Some(6000000)).unwrap();
        link_emi_installment(&conn, "tx_2", "inst_1", Some("acme electronics"), Some(12), Some(6000000)).unwrap();

        let group1: String = conn.query_row("SELECT emi_group_id FROM transactions WHERE id = 'tx_1'", [], |r| r.get(0)).unwrap();
        let group2: String = conn.query_row("SELECT emi_group_id FROM transactions WHERE id = 'tx_2'", [], |r| r.get(0)).unwrap();
        assert_eq!(group1, group2, "installments of the same EMI purchase must share emi_group_id");
        assert!(!group1.is_empty());
    }

    /// Doc 30 TASK-TXN-012 acceptance test.
    #[test]
    fn test_emi_parent_transaction_linked() {
        let conn = setup_db();
        insert_tx(&conn, "tx_1", "2026-01-15 10:00:00", 500000);
        insert_tx(&conn, "tx_2", "2026-02-15 10:00:00", 500000);

        link_emi_installment(&conn, "tx_1", "inst_1", Some("acme electronics"), Some(12), Some(6000000)).unwrap();
        link_emi_installment(&conn, "tx_2", "inst_1", Some("acme electronics"), Some(12), Some(6000000)).unwrap();

        let parent: Option<String> = conn.query_row("SELECT parent_transaction_id FROM transactions WHERE id = 'tx_2'", [], |r| r.get(0)).unwrap();
        assert_eq!(parent, Some("tx_1".to_string()), "the later installment must link back to the earlier (origination) one");

        let subtype: String = conn.query_row("SELECT transaction_subtype FROM transactions WHERE id = 'tx_2'", [], |r| r.get(0)).unwrap();
        assert_eq!(subtype, "emi_installment");
    }

    /// Doc 30 TASK-TXN-012 acceptance test.
    #[test]
    fn test_non_emi_transaction_has_null_emi_group() {
        let conn = setup_db();
        insert_tx(&conn, "tx_1", "2026-01-15 10:00:00", 50000);

        // No EMI fields provided -- must be a no-op.
        link_emi_installment(&conn, "tx_1", "inst_1", None, None, None).unwrap();

        let group: Option<String> = conn.query_row("SELECT emi_group_id FROM transactions WHERE id = 'tx_1'", [], |r| r.get(0)).unwrap();
        assert_eq!(group, None);
    }
}
