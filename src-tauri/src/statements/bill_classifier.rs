//! Identifies which statements represent bills with an amount due.
//!
//! A credit-card statement carries a due date and a payable amount; a savings
//! account statement does not. Distinguishing them is what allows an upcoming
//! bill to be surfaced without inventing one for every account.
use crate::statements::metadata_extractor::StatementMetadata;
use anyhow::Result;
use chrono::{Datelike, NaiveDate, Utc};
use tauri::Emitter;

/// Classifies a statement as a bill and updates its instrument.
pub async fn classify_and_update<R: tauri::Runtime>(
    instrument_id: &str,
    statement_id: &str,
    meta: &StatementMetadata,
    pool: &deadpool_sqlite::Pool,
    app_handle: Option<&tauri::AppHandle<R>>,
) -> Result<bool> {
    let is_upcoming = evaluate_upcoming_bill(meta);

    if is_upcoming {
        tracing::info!(
            "Upcoming bill detected for instrument_id='{}' statement_id='{}' due='{:?}'",
            instrument_id,
            statement_id,
            meta.due_date
        );

        update_instrument_bill_fields(instrument_id, meta, pool).await?;

        let payload = serde_json::json!({
            "statement_id": statement_id,
            "instrument_id": instrument_id,
            "due_date": meta.due_date,
            "current_balance": meta.current_balance,
            "minimum_due": meta.minimum_due,
        });

        if let Some(handle) = app_handle {
            if let Err(e) = handle.emit(crate::statements::events::UPCOMING_BILL_SET, &payload) {
                tracing::warn!("Failed to emit upcoming_bill_set event: {}", e);
            }
        }

        crate::statements::events::emit(crate::statements::events::UPCOMING_BILL_SET, payload);
    } else {
        tracing::info!(
            "Historical statement for instrument_id='{}' statement_id='{}' — no instrument update",
            instrument_id,
            statement_id
        );
    }

    Ok(is_upcoming)
}

/// Whether a statement represents a bill with an amount due.
///
/// A credit-card statement carries a due date and a payable amount; a savings
/// account statement does not. Distinguishing them prevents inventing an upcoming
/// bill for every account the user holds.
pub fn evaluate_upcoming_bill(meta: &StatementMetadata) -> bool {
    let today = Utc::now().date_naive();

    let period_end = match meta
        .billing_period_end
        .as_deref()
        .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
    {
        Some(d) => d,
        None => {
            tracing::debug!("evaluate_upcoming_bill: billing_period_end absent or unparseable");
            return false;
        }
    };

    let due_date = match meta
        .due_date
        .as_deref()
        .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
    {
        Some(d) => d,
        None => {
            tracing::debug!("evaluate_upcoming_bill: due_date absent or unparseable");
            return false;
        }
    };

    let current_year = today.year();
    let current_month = today.month();

    let (prior_year, prior_month) = if current_month == 1 {
        (current_year - 1, 12u32)
    } else {
        (current_year, current_month - 1)
    };

    let period_end_in_current_or_prior = (period_end.year() == current_year
        && period_end.month() == current_month)
        || (period_end.year() == prior_year && period_end.month() == prior_month);

    if !period_end_in_current_or_prior {
        tracing::debug!(
            "evaluate_upcoming_bill: billing_period_end {:?} is not in current ({}/{}) or prior ({}/{}) month",
            period_end,
            current_month,
            current_year,
            prior_month,
            prior_year
        );
        return false;
    }

    if due_date <= today {
        tracing::debug!(
            "evaluate_upcoming_bill: due_date {:?} is not in the future (today={:?})",
            due_date,
            today
        );
        return false;
    }

    tracing::debug!(
        "evaluate_upcoming_bill: upcoming bill confirmed — period_end={:?} due_date={:?} today={:?}",
        period_end,
        due_date,
        today
    );
    true
}

/// Writes due-date and amount fields onto the instrument.
async fn update_instrument_bill_fields(
    instrument_id: &str,
    meta: &StatementMetadata,
    pool: &deadpool_sqlite::Pool,
) -> Result<()> {
    let inst_id = instrument_id.to_string();
    let due_date = meta.due_date.clone();
    let minimum_due = meta.minimum_due;
    let current_balance = meta.current_balance;

    let conn = pool.get().await?;
    conn.interact(move |c| {
        c.execute(
            "UPDATE instruments \
             SET statement_due_date = ?, \
                 minimum_due = ?, \
                 current_balance = ?, \
                 updated_at = datetime('now') \
             WHERE id = ?",
            rusqlite::params![due_date, minimum_due, current_balance, inst_id],
        )
    })
    .await
    .map_err(|e| anyhow::anyhow!("DB interact error (update_instrument_bill_fields): {}", e))??;

    tracing::info!(
        "Updated instrument '{}': statement_due_date={:?} minimum_due={:?} current_balance={:?}",
        instrument_id,
        meta.due_date,
        meta.minimum_due,
        meta.current_balance
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_meta(
        billing_period_end: Option<&str>,
        due_date: Option<&str>,
        current_balance: Option<i64>,
        minimum_due: Option<i64>,
    ) -> StatementMetadata {
        crate::statements::metadata_extractor::StatementMetadata {
            billing_period_start: None,
            billing_period_end: billing_period_end.map(|s| s.to_string()),
            due_date: due_date.map(|s| s.to_string()),
            current_balance,
            minimum_due,
            issuer_name: None,
            masked_identifier: None,
            network: None,
            rewards_summary_json: None,
            statement_date: None,
        }
    }

    #[tokio::test]
    async fn test_upcoming_bill_detected_and_instrument_updated() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("test.db");
        let pool = crate::db::init_db(db_path).await.unwrap();

        let conn = pool.get().await.unwrap();
        conn.interact(|c| {
            c.execute(
                "INSERT INTO instruments (id, type, issuer_name, masked_identifier) \
                 VALUES ('inst_bill', 'credit_card', 'HDFC', '7777')",
                [],
            )
            .unwrap();
        })
        .await
        .unwrap();

        let today = Utc::now().date_naive();
        let period_end = NaiveDate::from_ymd_opt(today.year(), today.month(), 1)
            .unwrap()
            .format("%Y-%m-%d")
            .to_string();

        let due = today + chrono::Duration::days(30);
        let due_str = due.format("%Y-%m-%d").to_string();

        let meta = make_meta(
            Some(&period_end),
            Some(&due_str),
            Some(500_000),
            Some(25_000),
        );

        assert!(
            evaluate_upcoming_bill(&meta),
            "Statement with current-month period_end and future due_date must be upcoming"
        );

        let is_upcoming = classify_and_update(
            "inst_bill",
            "stmt_bill",
            &meta,
            &pool,
            None::<&tauri::AppHandle>,
        )
        .await
        .unwrap();
        assert!(is_upcoming);

        let conn2 = pool.get().await.unwrap();
        let (due_date_col, min_due_col, cur_bal_col): (Option<String>, Option<i64>, Option<i64>) =
            conn2
                .interact(|c| {
                    c.query_row(
                        "SELECT statement_due_date, minimum_due, current_balance \
                         FROM instruments WHERE id = ?",
                        ["inst_bill"],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                    )
                })
                .await
                .unwrap()
                .unwrap();

        assert_eq!(
            due_date_col.as_deref(),
            Some(due_str.as_str()),
            "statement_due_date must be updated"
        );
        assert_eq!(min_due_col, Some(25_000), "minimum_due must be updated");
        assert_eq!(
            cur_bal_col,
            Some(500_000),
            "current_balance must be updated"
        );
    }

    #[test]
    fn test_past_statement_not_marked_upcoming() {
        let today = Utc::now().date_naive();
        let old_period_end = today - chrono::Duration::days(180);
        let old_due_date = today - chrono::Duration::days(150);

        let meta = make_meta(
            Some(&old_period_end.format("%Y-%m-%d").to_string()),
            Some(&old_due_date.format("%Y-%m-%d").to_string()),
            Some(100_000),
            Some(10_000),
        );

        assert!(
            !evaluate_upcoming_bill(&meta),
            "Historical statement with past period_end and past due_date must NOT be upcoming"
        );
    }

    #[test]
    fn test_upcoming_bill_missing_due_date_returns_false() {
        let today = Utc::now().date_naive();
        let period_end = today.format("%Y-%m-%d").to_string();
        let meta = make_meta(Some(&period_end), None, Some(100_000), None);
        assert!(
            !evaluate_upcoming_bill(&meta),
            "Missing due_date must yield false"
        );
    }

    #[test]
    fn test_upcoming_bill_missing_period_end_returns_false() {
        let today = Utc::now().date_naive();
        let due = today + chrono::Duration::days(15);
        let meta = make_meta(None, Some(&due.format("%Y-%m-%d").to_string()), None, None);
        assert!(
            !evaluate_upcoming_bill(&meta),
            "Missing billing_period_end must yield false"
        );
    }

    #[test]
    fn test_upcoming_bill_prior_month_period_end_future_due_date() {
        let today = Utc::now().date_naive();
        let (prior_year, prior_month) = if today.month() == 1 {
            (today.year() - 1, 12u32)
        } else {
            (today.year(), today.month() - 1)
        };
        let period_end = NaiveDate::from_ymd_opt(prior_year, prior_month, 15)
            .unwrap()
            .format("%Y-%m-%d")
            .to_string();
        let future_due = (today + chrono::Duration::days(10))
            .format("%Y-%m-%d")
            .to_string();
        let meta = make_meta(
            Some(&period_end),
            Some(&future_due),
            Some(200_000),
            Some(5_000),
        );
        assert!(
            evaluate_upcoming_bill(&meta),
            "Prior month billing_period_end + future due_date must be upcoming"
        );
    }

    #[test]
    fn test_future_due_date_required() {
        let today = Utc::now().date_naive();
        let valid_period_end = today.format("%Y-%m-%d").to_string();

        let future_due = (today + chrono::Duration::days(5))
            .format("%Y-%m-%d")
            .to_string();
        assert!(evaluate_upcoming_bill(&make_meta(
            Some(&valid_period_end),
            Some(&future_due),
            None,
            None
        )));

        let past_due = (today - chrono::Duration::days(5))
            .format("%Y-%m-%d")
            .to_string();
        assert!(!evaluate_upcoming_bill(&make_meta(
            Some(&valid_period_end),
            Some(&past_due),
            None,
            None
        )));

        assert!(!evaluate_upcoming_bill(&make_meta(
            Some(&valid_period_end),
            None,
            None,
            None
        )));
    }

    #[test]
    fn test_upcoming_bill_due_today_is_not_future() {
        let today = Utc::now().date_naive();
        let period_end = today.format("%Y-%m-%d").to_string();
        let meta = make_meta(
            Some(&period_end),
            Some(&today.format("%Y-%m-%d").to_string()),
            None,
            None,
        );
        assert!(
            !evaluate_upcoming_bill(&meta),
            "due_date == today must NOT be upcoming"
        );
    }
}
