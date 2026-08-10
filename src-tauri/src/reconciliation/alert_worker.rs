//! Raises spending alerts and flags missing data.
//!
//! Two jobs share this module. Threshold alerts fire when spending crosses a
//! budget boundary. The missing-data loop watches for the absence of expected
//! input -- a statement period with no statement, an account that has gone quiet
//! -- which is the failure mode a transaction-driven system cannot otherwise
//! notice, since nothing arriving generates no event.
use crate::db::alerts::{insert_alert, Alert};
use crate::db::transactions::{
    get_category_spend_current_month, get_global_spend_current_month,
    get_trailing_30_day_merchant_average,
};
use crate::ipc::events::{emit_event, AppEvent};
use anyhow::Result;
use chrono::Datelike;
use deadpool_sqlite::Pool;
use rusqlite::OptionalExtension;
use tauri::AppHandle;

#[derive(serde::Serialize, Clone)]
pub struct AlertPayload {
    pub transaction_id: String,
    pub alert_type: String,
    pub message: String,
}

/// The user's global monthly spending limit, if set.
fn global_monthly_limit(conn: &rusqlite::Connection) -> Option<f64> {
    conn.query_row(
        "SELECT spending_limit_monthly FROM local_profile WHERE id = 1",
        [],
        |row| row.get::<_, Option<f64>>(0),
    )
    .ok()
    .flatten()
    .filter(|&v| v > 0.0)
}

/// A category's monthly budget, if set.
fn category_monthly_limit(conn: &rusqlite::Connection, category_id: &str) -> Option<f64> {
    conn.query_row(
        "SELECT monthly_budget_minor FROM categories WHERE id = ?1",
        rusqlite::params![category_id],
        |row| row.get::<_, Option<i64>>(0),
    )
    .ok()
    .flatten()
    .map(|minor| minor as f64 / 100.0)
    .filter(|&v| v > 0.0)
}

/// Which threshold levels the user has enabled.
fn enabled_threshold_levels(conn: &rusqlite::Connection) -> Vec<i64> {
    let json: Option<String> = conn
        .query_row(
            "SELECT limit_thresholds FROM local_profile WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .ok()
        .flatten();

    let mut levels: Vec<i64> = json
        .and_then(|j| serde_json::from_str::<Vec<f64>>(&j).ok())
        .map(|arr| arr.into_iter().map(|v| v as i64).collect())
        .unwrap_or_else(|| vec![80, 90, 100]);

    levels.sort_unstable();
    levels.dedup();
    levels
}

/// Whether this threshold has already fired this month.
///
/// The idempotency guard: without it every subsequent transaction over the limit
/// would fire the alert again, turning one breach into a stream of notifications.
fn threshold_already_fired_this_month(
    conn: &rusqlite::Connection,
    alert_key: &str,
    month_start: &str,
) -> Result<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM alerts WHERE type = ?1 AND created_at >= ?2 LIMIT 1",
            rusqlite::params![alert_key, month_start],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

/// Records that a threshold fired, so it does not fire again this month.
fn record_threshold_fired(
    conn: &rusqlite::Connection,
    alert_key: &str,
    message: &str,
) -> Result<()> {
    insert_alert(
        conn,
        &Alert {
            alert_id: uuid::Uuid::new_v4().to_string(),
            alert_type: alert_key.to_string(),
            message: message.to_string(),
            related_cluster_id: None,
            status: "fired".to_string(),
            created_at: None,
            updated_at: None,
        },
    )
}

#[allow(clippy::too_many_arguments)]
/// Evaluates spend against each enabled threshold and fires those newly crossed.
///
/// The epsilon in the comparison absorbs floating-point error, so spending exactly
/// at a boundary reliably triggers rather than depending on representation.
///
/// A missing limit means no budget is configured, which yields no alerts rather
/// than an alert about an undefined limit.
fn check_threshold_bands(
    conn: &rusqlite::Connection,
    tx_id: &str,
    scope_key: &str,
    label: &str,
    spend: f64,
    limit: Option<f64>,
    thresholds: &[i64],
    month_start: &str,
) -> Result<Vec<AlertPayload>> {
    let Some(limit) = limit else {
        return Ok(Vec::new());
    };
    let pct = (spend / limit) * 100.0;

    let mut fired = Vec::new();
    for &level in thresholds {
        if pct + 1e-9 < level as f64 {
            continue;
        }
        let alert_key = format!("{scope_key}_{level}");
        if threshold_already_fired_this_month(conn, &alert_key, month_start)? {
            continue;
        }
        let message = if level >= 100 {
            format!("{label} fully exhausted (100%+)")
        } else {
            format!("{label} at {level}% of limit")
        };
        record_threshold_fired(conn, &alert_key, &message)?;
        fired.push(AlertPayload {
            transaction_id: tx_id.to_string(),
            alert_type: alert_key,
            message,
        });
    }
    Ok(fired)
}

/// Evaluates alerts for newly ingested observations.
pub async fn evaluate_alerts_for_observations<R: tauri::Runtime>(
    pool: Pool,
    app_handle: AppHandle<R>,
    observation_ids: Vec<String>,
) -> Result<()> {
    if observation_ids.is_empty() {
        return Ok(());
    }

    pool.get()
        .await?
        .interact(move |conn| -> Result<()> {
            evaluate_alerts_internal(conn, Some(app_handle), observation_ids)
        })
        .await
        .map_err(|e| anyhow::anyhow!("Interact error: {:?}", e))??;

    Ok(())
}

/// Warns about subscription charges due soon.
pub fn check_upcoming_subscriptions(
    conn: &rusqlite::Connection,
    app_handle: Option<&AppHandle>,
    reference_date: &chrono::NaiveDate,
) -> Result<()> {
    let horizon = *reference_date + chrono::Duration::days(3);
    let horizon_str = horizon.format("%Y-%m-%d").to_string();
    let today_str = reference_date.format("%Y-%m-%d").to_string();

    let mut stmt = conn.prepare(
        "SELECT id, merchant_entity_id, amount_minor, next_billing_date, next_predicted_date
         FROM recurring_payments
         WHERE status NOT IN ('cancelled', 'paused')
           AND (
             (next_billing_date IS NOT NULL AND next_billing_date >= ?1 AND next_billing_date <= ?2)
             OR
             (next_predicted_date IS NOT NULL AND next_predicted_date >= ?1 AND next_predicted_date <= ?2)
           )",
    )?;

    let rows = stmt.query_map(rusqlite::params![today_str, horizon_str], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<i64>>(2)?,
        ))
    })?;

    for row in rows {
        let (rp_id, merchant_id, amount_minor) = row?;
        let amount_display = amount_minor.unwrap_or(0) as f64 / 100.0;
        let alert = AlertPayload {
            transaction_id: rp_id.clone(),
            alert_type: "upcoming_subscription".to_string(),
            message: format!(
                "Subscription renewal due within 3 days: {:?} (₹{:.2})",
                merchant_id, amount_display
            ),
        };
        if let Some(app) = app_handle {
            let _ = emit_event(app, AppEvent::AlertThresholdCrossed, alert);
        }
    }
    Ok(())
}

/// Core alert evaluation over the current spending state.
pub fn evaluate_alerts_internal<R: tauri::Runtime>(
    conn: &rusqlite::Connection,
    app_handle: Option<AppHandle<R>>,
    observation_ids: Vec<String>,
) -> Result<()> {
    type AlertTxInfo = (String, i64, Option<String>, Option<String>, Option<String>);

    for obs_id in observation_ids {
        let tx_info: rusqlite::Result<AlertTxInfo> = conn.query_row(
            "SELECT t.id, t.amount_minor, t.category_id, t.merchant_entity_id, t.best_event_time 
             FROM transactions t
             JOIN transaction_observations obs ON obs.canonical_transaction_id = t.id
             WHERE obs.id = ?1 AND t.alert_fired = 0 AND t.direction = 'debit'",
            rusqlite::params![obs_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        );

        let (tx_id, amount_minor, category_id, merchant_raw, event_time_str) = match tx_info {
            Ok(data) => data,
            Err(_) => continue,
        };

        let event_time = if let Some(et_str) = event_time_str {
            chrono::NaiveDateTime::parse_from_str(&et_str, "%Y-%m-%d %H:%M:%S")
                .unwrap_or_else(|_| chrono::Utc::now().naive_utc())
        } else {
            chrono::Utc::now().naive_utc()
        };

        let mut fired_alerts = Vec::new();
        let month_start = format!(
            "{}-{:02}-01 00:00:00",
            event_time.date().year(),
            event_time.date().month()
        );
        let thresholds = enabled_threshold_levels(conn);

        if let Some(cat) = &category_id {
            let category_spend =
                get_category_spend_current_month(conn, cat, &event_time).unwrap_or(0.0);
            let limit = category_monthly_limit(conn, cat);
            fired_alerts.extend(check_threshold_bands(
                conn,
                &tx_id,
                &format!("category_budget_{cat}"),
                &format!("Category '{cat}' monthly budget"),
                category_spend,
                limit,
                &thresholds,
                &month_start,
            )?);
        }

        let global_spend = get_global_spend_current_month(conn, &event_time).unwrap_or(0.0);
        let global_limit = global_monthly_limit(conn);
        fired_alerts.extend(check_threshold_bands(
            conn,
            &tx_id,
            "global_budget",
            "Global monthly spending",
            global_spend,
            global_limit,
            &thresholds,
            &month_start,
        )?);

        if let Some(merchant) = &merchant_raw {
            let average =
                get_trailing_30_day_merchant_average(conn, merchant, &event_time).unwrap_or(0.0);
            let amount = amount_minor as f64;
            if average > 0.0 && amount > average * 3.0 {
                fired_alerts.push(AlertPayload {
                    transaction_id: tx_id.clone(),
                    alert_type: "merchant_spike".to_string(),
                    message: format!("Unusual spend amount at {}", merchant),
                });
            }
        }

        if !fired_alerts.is_empty() {
            conn.execute(
                "UPDATE transactions SET alert_fired = 1 WHERE id = ?1",
                rusqlite::params![tx_id],
            )?;

            if let Some(app) = &app_handle {
                for alert in fired_alerts {
                    if alert.alert_type.starts_with("global_budget_") {
                        crate::notifications::send_notification(
                            app,
                            crate::notifications::NotificationKind::SpendingLimitThreshold,
                            "Spending Limit Alert",
                            &alert.message,
                            None,
                        );
                    }
                    let _ = emit_event(app, AppEvent::AlertThresholdCrossed, alert);
                }
            }
        }
    }
    Ok(())
}

use std::time::Duration;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Polls for missing expected data, such as an absent statement period.
///
/// Catches the failure a transaction-driven system cannot see on its own: nothing
/// arriving generates no event, so absence has to be actively looked for.
pub async fn start_missing_data_polling_loop(
    app_handle: AppHandle,
    pool: Pool,
    cancel_token: CancellationToken,
) {
    tracing::info!("Starting Missing Data Alert Worker loop...");
    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                tracing::info!("Missing Data Alert Worker loop cancelled");
                break;
            }
            _ = tokio::time::sleep(Duration::from_secs(60 * 60)) => {
                let pool_clone = pool.clone();
                let app_handle_clone = app_handle.clone();
                if let Err(e) = evaluate_missing_data_alerts(pool_clone, app_handle_clone).await {
                    tracing::error!("Failed to evaluate missing data alerts: {}", e);
                }
            }
        }
    }
}

/// Evaluates and raises missing-data alerts.
pub async fn evaluate_missing_data_alerts(pool: Pool, app_handle: AppHandle) -> Result<()> {
    let conn = pool.get().await?;
    let alerts_to_create = conn
        .interact(move |c| -> Result<Vec<Alert>> {
            let now = chrono::Utc::now().naive_utc();
            let threshold = now - chrono::Duration::hours(2);
            let threshold_str = threshold.format("%Y-%m-%d %H:%M:%S").to_string();

            let mut stmt = c.prepare(
                "SELECT c.id, o.event_time, i.issuer_name
             FROM reconciliation_clusters c
             JOIN reconciliation_cluster_members m ON m.cluster_id = c.id AND m.member_role = 'incoming'
             JOIN transaction_observations o ON m.observation_id = o.id
             JOIN instruments i ON o.instrument_id = i.id
             WHERE c.cluster_status = 'open'
               AND o.confidence_score < 0.5
               AND o.event_time < ?1
               AND NOT EXISTS (
                   SELECT 1 FROM alerts a 
                   WHERE a.related_cluster_id = c.id 
                     AND a.type = 'SMS Offline'
               )",
            )?;

            let rows = stmt.query_map(rusqlite::params![threshold_str], |row| {
                let cluster_id: String = row.get(0)?;
                let event_time: Option<String> = row.get(1)?;
                let issuer_name: String = row.get(2)?;
                Ok((cluster_id, event_time, issuer_name))
            })?;

            let mut new_alerts = Vec::new();

            for row in rows {
                let (cluster_id, event_time_str, issuer_name) = row?;
                if let Some(et_str) = event_time_str {
                    if let Ok(event_time) =
                        chrono::NaiveDateTime::parse_from_str(&et_str, "%Y-%m-%d %H:%M:%S")
                    {
                        let mut sync_stmt = c.prepare(
                            "SELECT last_synced_at FROM sync_metadata WHERE bank_name = ?1",
                        )?;

                        let last_synced_at = sync_stmt
                            .query_row(rusqlite::params![issuer_name], |r| {
                                let ds: String = r.get(0)?;
                                Ok(
                                    chrono::NaiveDateTime::parse_from_str(&ds, "%Y-%m-%d %H:%M:%S")
                                        .unwrap_or_default(),
                                )
                            })
                            .optional()?;

                        let mut should_alert = false;
                        if let Some(sync_time) = last_synced_at {
                            if sync_time < event_time {
                                should_alert = true;
                            }
                        } else {
                            should_alert = true;
                        }

                        if should_alert {
                            new_alerts.push(Alert {
                                alert_id: Uuid::new_v4().to_string(),
                                alert_type: "SMS Offline".to_string(),
                                message: format!("Missing data from bank: {}", issuer_name),
                                related_cluster_id: Some(cluster_id),
                                status: "pending".to_string(),
                                created_at: None,
                                updated_at: None,
                            });
                        }
                    }
                }
            }

            for alert in &new_alerts {
                insert_alert(c, alert)?;
            }

            Ok(new_alerts)
        })
        .await
        .map_err(|e| anyhow::anyhow!("DB interact error: {}", e))??;

    for alert in alerts_to_create {
        let _ = emit_event(
            &app_handle,
            AppEvent::AlertThresholdCrossed,
            AlertPayload {
                transaction_id: alert.related_cluster_id.unwrap_or_default(),
                alert_type: alert.alert_type,
                message: alert.message,
            },
        );
    }

    Ok(())
}
