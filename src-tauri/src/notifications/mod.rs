//! Native notifications and the rules governing when to send one.
//!
//! Restraint is the design: predicates decide whether a given event warrants
//! interrupting the user at all. Bill reminders fire on a fixed lead time, and a
//! bulk import deliberately does not notify per transaction.
use tauri::{AppHandle, Manager, Runtime};

pub const DEFAULT_TRANSACTION_NOTIFICATION_THRESHOLD_MINOR: i64 = 100_000;

pub const NETWORK_DISCLOSURE_CONSENT_EVENT_TYPE: &str = "network_disclosure_acknowledged";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationKind {
    TransactionAboveThreshold,
    SpendingLimitThreshold,
    UpcomingBillDue,
}

impl NotificationKind {
    /// The in-app route this notification should open when clicked.
    pub fn deep_link_route(&self, instrument_id: Option<&str>) -> String {
        match self {
            Self::TransactionAboveThreshold => "/transactions".to_string(),
            Self::SpendingLimitThreshold => "/".to_string(),
            Self::UpcomingBillDue => match instrument_id {
                Some(id) => format!("/instruments/{id}"),
                None => "/instruments".to_string(),
            },
        }
    }
}

/// Whether a transaction is large enough to warrant notifying.
pub fn should_notify_transaction(amount_minor: i64, threshold_minor: i64) -> bool {
    amount_minor >= threshold_minor
}

/// Whether notification permission may be requested yet.
///
/// Gated on the network disclosure having been acknowledged, so the OS prompt
/// never appears before the user has been told what the app does.
pub fn should_request_permission(network_disclosure_acknowledged: bool) -> bool {
    network_disclosure_acknowledged
}

/// Whether a due date is exactly three days away.
pub fn is_three_days_before_due(due_date: chrono::NaiveDate, today: chrono::NaiveDate) -> bool {
    (due_date - today).num_days() == 3
}

/// Sends a native notification.
pub fn send_notification<R: Runtime>(
    app: &AppHandle<R>,
    kind: NotificationKind,
    title: &str,
    body: &str,
    instrument_id: Option<&str>,
) {
    let Some(notification) = app.try_state::<tauri_plugin_notification::Notification<R>>() else {
        tracing::debug!(
            "Notification plugin not registered -- skipping native notification (kind={:?})",
            kind
        );
        return;
    };
    let route = kind.deep_link_route(instrument_id);
    let result = notification
        .builder()
        .title(title)
        .body(body)
        .extra("deep_link", route)
        .show();
    if let Err(e) = result {
        tracing::warn!("Failed to show native notification ({:?}): {}", kind, e);
    }
}

/// Requests notification permission, once disclosure has been acknowledged.
pub async fn request_permission_if_disclosed<R: Runtime>(
    app: &AppHandle<R>,
    pool: &deadpool_sqlite::Pool,
) {
    let Some(notification) = app.try_state::<tauri_plugin_notification::Notification<R>>() else {
        return;
    };
    let Ok(conn) = pool.get().await else {
        return;
    };
    let acknowledged = conn
        .interact(|c| {
            crate::auth::consent::has_active_consent(c, NETWORK_DISCLOSURE_CONSENT_EVENT_TYPE)
        })
        .await;
    if matches!(acknowledged, Ok(Ok(true))) && should_request_permission(true) {
        if let Err(e) = notification.request_permission() {
            tracing::warn!("Failed to request notification permission: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn test_notification_suppressed_below_threshold() {
        assert!(!should_notify_transaction(50_000, 100_000));
        assert!(should_notify_transaction(100_000, 100_000));
        assert!(should_notify_transaction(250_000, 100_000));
    }

    #[test]
    fn test_notification_deep_links_to_correct_view() {
        assert_eq!(
            NotificationKind::TransactionAboveThreshold.deep_link_route(None),
            "/transactions"
        );
        assert_eq!(
            NotificationKind::SpendingLimitThreshold.deep_link_route(None),
            "/"
        );
        assert_eq!(
            NotificationKind::UpcomingBillDue.deep_link_route(Some("inst_1")),
            "/instruments/inst_1"
        );
        assert_eq!(
            NotificationKind::UpcomingBillDue.deep_link_route(None),
            "/instruments"
        );
    }

    #[test]
    fn test_permission_requested_after_privacy_disclosure() {
        assert!(!should_request_permission(false));
        assert!(should_request_permission(true));
    }

    #[test]
    fn test_is_three_days_before_due() {
        let today = NaiveDate::from_ymd_opt(2026, 7, 17).unwrap();
        assert!(is_three_days_before_due(
            NaiveDate::from_ymd_opt(2026, 7, 20).unwrap(),
            today
        ));
        assert!(!is_three_days_before_due(
            NaiveDate::from_ymd_opt(2026, 7, 19).unwrap(),
            today
        ));
        assert!(!is_three_days_before_due(
            NaiveDate::from_ymd_opt(2026, 7, 21).unwrap(),
            today
        ));
        assert!(!is_three_days_before_due(today, today));
    }

    #[test]
    fn test_send_notification_is_a_safe_noop_without_the_plugin_registered() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap()
            .handle()
            .clone();
        send_notification(
            &app,
            NotificationKind::TransactionAboveThreshold,
            "title",
            "body",
            None,
        );
    }
}
