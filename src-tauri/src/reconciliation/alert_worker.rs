use crate::db::transactions::{
    get_category_spend_current_month, get_global_spend_current_month,
    get_trailing_30_day_merchant_average,
};
use crate::ipc::events::{emit_event, AppEvent};
use anyhow::Result;
use deadpool_sqlite::Pool;
use tauri::AppHandle;

/// Configurable monthly budget limits (minor units, i.e. paise/cents).
/// These defaults represent ₹5,000 global and ₹500 per-category.
/// 100% threshold values; 80% and 90% are computed proportionally.
const GLOBAL_MONTHLY_BUDGET_MINOR: f64 = 5_000.0;
const CATEGORY_MONTHLY_BUDGET_MINOR: f64 = 500.0;

#[derive(serde::Serialize, Clone)]
pub struct AlertPayload {
    pub transaction_id: String,
    pub alert_type: String,
    pub message: String,
}

pub async fn evaluate_alerts_for_observations<R: tauri::Runtime>(
    pool: Pool,
    app_handle: AppHandle<R>,
    observation_ids: Vec<String>,
) -> Result<()> {
    if observation_ids.is_empty() {
        return Ok(());
    }

    pool
        .get()
        .await?
        .interact(move |conn| -> Result<()> {
            evaluate_alerts_internal(conn, Some(app_handle), observation_ids)
        })
        .await
        .map_err(|e| anyhow::anyhow!("Interact error: {:?}", e))??;

    Ok(())
}

/// Checks for upcoming subscription renewals within the next 3 days and fires alerts.
/// Queries `recurring_payments.next_billing_date` or `next_predicted_date` ≤ today + 3 days.
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

        // 1. Per-category budget threshold bands: 80%, 90%, 100%
        if let Some(cat) = &category_id {
            let category_spend =
                get_category_spend_current_month(conn, cat, &event_time).unwrap_or(0.0);
            let pct = category_spend / CATEGORY_MONTHLY_BUDGET_MINOR;
            let alert_type = if pct >= 1.0 {
                Some((
                    "category_budget_100",
                    format!("Category '{}' monthly budget fully exhausted (100%+)", cat),
                ))
            } else if pct >= 0.90 {
                Some((
                    "category_budget_90",
                    format!("Category '{}' budget at 90%", cat),
                ))
            } else if pct >= 0.80 {
                Some((
                    "category_budget_80",
                    format!("Category '{}' budget at 80%", cat),
                ))
            } else {
                None
            };
            if let Some((atype, amsg)) = alert_type {
                fired_alerts.push(AlertPayload {
                    transaction_id: tx_id.clone(),
                    alert_type: atype.to_string(),
                    message: amsg,
                });
            }
        }

        // 2. Global monthly spending threshold bands: 80%, 90%, 100%
        let global_spend = get_global_spend_current_month(conn, &event_time).unwrap_or(0.0);
        let global_pct = global_spend / GLOBAL_MONTHLY_BUDGET_MINOR;
        let global_alert = if global_pct >= 1.0 {
            Some((
                "global_budget_100",
                "Global monthly spending limit fully exhausted (100%+)".to_string(),
            ))
        } else if global_pct >= 0.90 {
            Some((
                "global_budget_90",
                "Global monthly spending at 90% of limit".to_string(),
            ))
        } else if global_pct >= 0.80 {
            Some((
                "global_budget_80",
                "Global monthly spending at 80% of limit".to_string(),
            ))
        } else {
            None
        };
        if let Some((atype, amsg)) = global_alert {
            fired_alerts.push(AlertPayload {
                transaction_id: tx_id.clone(),
                alert_type: atype.to_string(),
                message: amsg,
            });
        }

        // 3. Merchant Spike Anomaly (unusual spend: current > 3x trailing 30-day average)
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
                    let _ = emit_event(app, AppEvent::AlertThresholdCrossed, alert);
                }
            }
        }
    }
    Ok(())
}

use crate::db::alerts::{insert_alert, Alert};
use rusqlite::OptionalExtension;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

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
                            // No sync metadata for this bank yet
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
