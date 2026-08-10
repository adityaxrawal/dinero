//! The licence state transition rules.
//!
//! Isolated as a pure function so every path -- trial expiry, payment failure,
//! grace elapsing, reactivation -- is exhaustively testable without a database
//! or a network.
use anyhow::{bail, Result};
use rusqlite::Connection;

use super::state::{get_license_state, LicenseStatus};

/// Whether a state transition is permitted.
///
/// Enumerated explicitly so an invalid jump -- locked straight back to active
/// without revalidation -- cannot occur through an unchecked write.
fn is_legal_transition(from: &LicenseStatus, to: &LicenseStatus) -> bool {
    use LicenseStatus::*;
    matches!(
        (from, to),
        (AnonymousEval, Trial)
            | (Trial, Active)
            | (Active, Grace)
            | (Grace, Active)
            | (Grace, Locked)
            | (Active, Locked)
            | (Trial, Locked)
            | (AnonymousEval, Locked)
    )
}

/// Applies a state transition if it is legal.
pub fn transition(conn: &Connection, to: LicenseStatus) -> Result<()> {
    let current = get_license_state(conn)?;
    let from = current
        .map(|s| s.subscription_status_cached)
        .unwrap_or(LicenseStatus::AnonymousEval);

    if from == to {
        return Ok(());
    }
    if !is_legal_transition(&from, &to) {
        bail!("Illegal license state transition: {:?} -> {:?}", from, to);
    }

    conn.execute(
        "UPDATE license_state SET subscription_status_cached = ?1, updated_at = CURRENT_TIMESTAMP WHERE id = 1",
        rusqlite::params![to.as_str()],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::licensing::state::{upsert_license_state, LicenseStateRow};
    use chrono::Utc;

    fn seed_state(conn: &Connection, status: LicenseStatus) {
        let now = Utc::now();
        upsert_license_state(
            conn,
            &LicenseStateRow {
                id: 1,
                license_jwt: "jwt".to_string(),
                subscription_status_cached: status,
                plan_id_cached: Some("pro".to_string()),
                current_period_end_cached: Some(now),
                jwt_expires_at: now,
                last_server_validated_at: Some(now),
                last_known_valid_time: now,
                device_fingerprint: Some("dev1".to_string()),
                source: "server_fresh".to_string(),
                billing_interval_cached: Some("monthly".to_string()),
            },
        )
        .unwrap();
    }

    #[test]
    fn test_license_state_machine_transitions() {
        let conn = crate::db::test_helpers::setup_test_db();

        seed_state(&conn, LicenseStatus::AnonymousEval);
        transition(&conn, LicenseStatus::Trial).unwrap();
        assert_eq!(
            get_license_state(&conn)
                .unwrap()
                .unwrap()
                .subscription_status_cached,
            LicenseStatus::Trial
        );

        transition(&conn, LicenseStatus::Active).unwrap();
        assert_eq!(
            get_license_state(&conn)
                .unwrap()
                .unwrap()
                .subscription_status_cached,
            LicenseStatus::Active
        );

        transition(&conn, LicenseStatus::Grace).unwrap();
        assert_eq!(
            get_license_state(&conn)
                .unwrap()
                .unwrap()
                .subscription_status_cached,
            LicenseStatus::Grace
        );

        transition(&conn, LicenseStatus::Active).unwrap();
        assert_eq!(
            get_license_state(&conn)
                .unwrap()
                .unwrap()
                .subscription_status_cached,
            LicenseStatus::Active
        );

        transition(&conn, LicenseStatus::Locked).unwrap();
        assert_eq!(
            get_license_state(&conn)
                .unwrap()
                .unwrap()
                .subscription_status_cached,
            LicenseStatus::Locked
        );

        let result = transition(&conn, LicenseStatus::Active);
        assert!(
            result.is_err(),
            "LOCKED -> ACTIVE must be rejected without a real reactivation"
        );

        assert_eq!(
            get_license_state(&conn)
                .unwrap()
                .unwrap()
                .subscription_status_cached,
            LicenseStatus::Locked
        );
    }

    #[test]
    fn transition_to_the_same_state_is_a_no_op() {
        let conn = crate::db::test_helpers::setup_test_db();
        seed_state(&conn, LicenseStatus::Active);
        transition(&conn, LicenseStatus::Active).unwrap();
        assert_eq!(
            get_license_state(&conn)
                .unwrap()
                .unwrap()
                .subscription_status_cached,
            LicenseStatus::Active
        );
    }

    #[test]
    fn grace_to_locked_is_legal() {
        let conn = crate::db::test_helpers::setup_test_db();
        seed_state(&conn, LicenseStatus::Grace);
        transition(&conn, LicenseStatus::Locked).unwrap();
        assert_eq!(
            get_license_state(&conn)
                .unwrap()
                .unwrap()
                .subscription_status_cached,
            LicenseStatus::Locked
        );
    }

    #[test]
    fn trial_to_grace_is_illegal() {
        let conn = crate::db::test_helpers::setup_test_db();
        seed_state(&conn, LicenseStatus::Trial);
        assert!(transition(&conn, LicenseStatus::Grace).is_err());
    }
}
