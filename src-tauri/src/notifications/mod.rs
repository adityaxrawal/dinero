//! TASK-DESK-002 (Doc 30 §12, Doc 29 §14): native macOS notifications via
//! Tauri's notification plugin (`UNUserNotificationCenter`), distinct from
//! the in-app toast/banner system (TASK-FE-018). Fires for four event
//! kinds: a new confirmed transaction above a user threshold, a
//! spending-limit threshold crossing, a statement password prompt timing
//! out, and an approaching bill due date (3 days before). Clicking a
//! notification foregrounds the app and deep-links to the relevant view.
//! macOS Do Not Disturb/Focus modes are respected automatically by the OS
//! -- no custom suppression logic is implemented or needed here.

use tauri::{AppHandle, Manager, Runtime};

/// Default per-transaction notification threshold (paise/minor units) used
/// until a settings UI exists to make this genuinely user-configurable --
/// avoids notification fatigue from every small transaction (Doc 30
/// TASK-DESK-002's own wording). Flagged, not built out further, in this
/// task's fix-log entry: no `local_profile` column or settings screen for
/// this exists yet.
pub const DEFAULT_TRANSACTION_NOTIFICATION_THRESHOLD_MINOR: i64 = 100_000; // ₹1,000

/// `consent_events.event_type` recorded when the user passes the onboarding
/// network-disclosure screen (Doc 18 §4.21a's generic consent table, reused
/// rather than adding a dedicated column for this one flag).
pub const NETWORK_DISCLOSURE_CONSENT_EVENT_TYPE: &str = "network_disclosure_acknowledged";

/// What a native notification represents, and where clicking it should
/// deep-link the user once the app is foregrounded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationKind {
    TransactionAboveThreshold,
    SpendingLimitThreshold,
    StatementPasswordTimeout,
    UpcomingBillDue,
}

impl NotificationKind {
    /// `instrument_id` is only used for `UpcomingBillDue`, where the
    /// reminder is naturally instrument-scoped. The other three kinds route
    /// to their general list view rather than one specific record: the
    /// canonical transaction id isn't known yet at the point a
    /// transaction-above-threshold notification fires during ingestion
    /// (only the pre-reconciliation observation id is), so linking to a
    /// list view that will contain it is honest rather than fabricating a
    /// wrong deep-link target.
    pub fn deep_link_route(&self, instrument_id: Option<&str>) -> String {
        match self {
            Self::TransactionAboveThreshold => "/transactions".to_string(),
            // Dashboard is the router's index route ("/"), not "/dashboard"
            // (Doc 30 TASK-FE-001, `routes/index.tsx`).
            Self::SpendingLimitThreshold => "/".to_string(),
            Self::StatementPasswordTimeout => "/statements".to_string(),
            Self::UpcomingBillDue => match instrument_id {
                Some(id) => format!("/instruments/{id}"),
                None => "/instruments".to_string(),
            },
        }
    }
}

/// Pure decision: should a newly confirmed transaction fire a native
/// notification? (Doc 30 acceptance: `test_notification_suppressed_below_threshold`.)
pub fn should_notify_transaction(amount_minor: i64, threshold_minor: i64) -> bool {
    amount_minor >= threshold_minor
}

/// Pure decision: is it appropriate to request OS notification permission
/// right now? Gated on the network/privacy disclosure screen having
/// already been acknowledged -- never requested proactively before the
/// user has seen it (Doc 30 acceptance:
/// `test_permission_requested_after_privacy_disclosure`).
pub fn should_request_permission(network_disclosure_acknowledged: bool) -> bool {
    network_disclosure_acknowledged
}

/// Pure decision: is `today` exactly 3 days before `due_date`? Doc 30: "an
/// approaching bill due date (3 days before, via a scheduled task)". Exact
/// equality (not a `<=` range) is what makes a once-daily scheduled check
/// self-deduplicating -- it becomes true on exactly one day per bill, so no
/// separate "already reminded" tracking is needed.
pub fn is_three_days_before_due(due_date: chrono::NaiveDate, today: chrono::NaiveDate) -> bool {
    (due_date - today).num_days() == 3
}

/// Fires a native OS notification. Best-effort in two senses: a failure
/// from the plugin itself (e.g. the user denied notification permission, or
/// Focus mode is suppressing it) is logged and dropped rather than
/// surfaced as an application error, matching this codebase's established
/// pattern for every other best-effort side channel (event emission, audit
/// logging); and if the notification plugin isn't registered at all (only
/// true in tests -- `tauri::test::mock_builder()` never registers it) this
/// looks it up via `try_state` rather than the plugin's own `NotificationExt`
/// convenience trait, which calls the panicking `state()` internally. That
/// split is deliberate: it lets this module's tests exercise the real
/// dispatch call sites (e.g. `handle_password_timeout`) without either
/// panicking or actually posting to the test machine's real notification
/// center.
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

/// Requests OS notification permission if (and only if) the network
/// disclosure has already been acknowledged. Called once at app startup;
/// a no-op on every launch before that consent exists, and idempotent
/// after it (macOS itself no-ops a repeat permission request once already
/// granted or denied). Same `try_state` reasoning as `send_notification`.
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

    /// Doc 30 TASK-DESK-002 acceptance: `test_notification_suppressed_below_threshold`.
    #[test]
    fn test_notification_suppressed_below_threshold() {
        assert!(!should_notify_transaction(50_000, 100_000));
        assert!(should_notify_transaction(100_000, 100_000));
        assert!(should_notify_transaction(250_000, 100_000));
    }

    /// Doc 30 TASK-DESK-002 acceptance: `test_notification_deep_links_to_correct_view`.
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
            NotificationKind::StatementPasswordTimeout.deep_link_route(None),
            "/statements"
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

    /// Doc 30 TASK-DESK-002 acceptance: `test_permission_requested_after_privacy_disclosure`.
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

    /// The notification plugin is never registered against
    /// `tauri::test::MockRuntime` (registering it would risk actually
    /// posting to the test machine's real notification center, the same
    /// category of platform side-effect the menu module's tests avoid for
    /// `muda`) -- `send_notification` must degrade to a safe no-op rather
    /// than panic in that case.
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
