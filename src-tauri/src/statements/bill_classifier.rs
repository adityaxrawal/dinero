use crate::statements::metadata_extractor::StatementMetadata;
use anyhow::Result;
use chrono::{Datelike, NaiveDate, Utc};
use tauri::Emitter;

/// Evaluates whether a statement represents the current active billing cycle
/// and updates instrument fields accordingly (Doc 10 §15).
///
/// An upcoming bill is detected when:
///   - billing_period_end is within the current month or immediately past month, AND
///   - due_date is in the future relative to now()
///
/// On detection, updates to `instruments`:
///   - instruments.statement_due_date = due_date
///   - instruments.minimum_due = minimum_due
///   - instruments.current_balance = current_balance
///
/// Then emits Tauri event: `statement.upcoming_bill_set`
///
/// If due_date is in the past → historical statement; no instrument fields updated (§15.2).
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

        // Update instruments table with statement fields
        update_instrument_bill_fields(instrument_id, meta, pool).await?;

        // Emit Tauri event `statement.upcoming_bill_set`
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
        // §15.2: Past statements are archived; instruments table is NOT updated
    }

    Ok(is_upcoming)
}

/// Pure evaluation logic: returns true if the statement qualifies as an upcoming bill.
/// Separated from DB/event side-effects for testability.
///
/// Conditions (Doc 10 §15.1):
///   1. `billing_period_end` is parseable AND falls within the current or immediately preceding month.
///   2. `due_date` is parseable AND is strictly in the future (after today UTC).
pub fn evaluate_upcoming_bill(meta: &StatementMetadata) -> bool {
    let today = Utc::now().date_naive();

    // Require both billing_period_end and due_date to be present
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

    // Condition 1: billing_period_end is in current month or prior month
    let current_year = today.year();
    let current_month = today.month();

    // Prior month (handle January → December of previous year)
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

    // Condition 2: due_date is strictly in the future
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

/// Updates the `instruments` row with current billing cycle fields (Doc 10 §15.1).
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates a StatementMetadata with given dates for testing.
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

    // ── test_upcoming_bill_detected_and_instrument_updated ────────────────────

    #[tokio::test]
    async fn test_upcoming_bill_detected_and_instrument_updated() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("test.db");
        let pool = crate::db::init_db(db_path).await.unwrap();

        // Seed instrument
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

        // Create metadata: billing_period_end = current month, due_date = next month (future)
        let today = Utc::now().date_naive();
        let period_end = NaiveDate::from_ymd_opt(today.year(), today.month(), 1)
            .unwrap()
            .format("%Y-%m-%d")
            .to_string();

        // due_date = 30 days from today (guaranteed future)
        let due = today + chrono::Duration::days(30);
        let due_str = due.format("%Y-%m-%d").to_string();

        let meta = make_meta(
            Some(&period_end),
            Some(&due_str),
            Some(500_000),
            Some(25_000),
        );

        // Must be detected as upcoming
        assert!(
            evaluate_upcoming_bill(&meta),
            "Statement with current-month period_end and future due_date must be upcoming"
        );

        // Run full classify_and_update pipeline
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

        // Verify instrument was updated
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

    // ── test_past_statement_not_marked_upcoming ───────────────────────────────

    #[test]
    fn test_past_statement_not_marked_upcoming() {
        // billing_period_end = 6 months ago, due_date = 5 months ago (both past)
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

    // ── Edge cases ────────────────────────────────────────────────────────────

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
        // Prior month period_end + future due_date → should be upcoming
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

    // ── test_future_due_date_required (Doc 30 TASK-STMT-007) ─────────────────
    // The exact acceptance-criteria name for the rule already exercised piecemeal
    // by test_upcoming_bill_due_today_is_not_future/missing_due_date_returns_false:
    // a valid, in-current/prior-month billing_period_end alone is never enough —
    // due_date must independently be present AND strictly future.

    #[test]
    fn test_future_due_date_required() {
        let today = Utc::now().date_naive();
        let valid_period_end = today.format("%Y-%m-%d").to_string();

        // Future due_date + valid period_end → upcoming.
        let future_due = (today + chrono::Duration::days(5))
            .format("%Y-%m-%d")
            .to_string();
        assert!(evaluate_upcoming_bill(&make_meta(
            Some(&valid_period_end),
            Some(&future_due),
            None,
            None
        )));

        // Past due_date, otherwise identical → not upcoming.
        let past_due = (today - chrono::Duration::days(5))
            .format("%Y-%m-%d")
            .to_string();
        assert!(!evaluate_upcoming_bill(&make_meta(
            Some(&valid_period_end),
            Some(&past_due),
            None,
            None
        )));

        // No due_date at all, otherwise identical → not upcoming.
        assert!(!evaluate_upcoming_bill(&make_meta(
            Some(&valid_period_end),
            None,
            None,
            None
        )));
    }

    #[test]
    fn test_upcoming_bill_due_today_is_not_future() {
        // due_date = today → not strictly in the future → must NOT be upcoming
        let today = Utc::now().date_naive();
        let period_end = today.format("%Y-%m-%d").to_string();
        let meta = make_meta(
            Some(&period_end),
            Some(&today.format("%Y-%m-%d").to_string()),
            None,
            None,
        );
        // due_date == today: not strictly > today → false
        assert!(
            !evaluate_upcoming_bill(&meta),
            "due_date == today must NOT be upcoming"
        );
    }
}
