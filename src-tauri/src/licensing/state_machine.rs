//! TASK-AUTH-009: Implement Local License State Machine.
//!
//! Document 03's authoritative state machine: `ANONYMOUS_EVAL`, `TRIAL`,
//! `ACTIVE`, `GRACE`, `LOCKED`, backed by the single-row `license_state`
//! table. This module is the one place that decides whether a *local*,
//! offline status change (network failure → grace, grace exhaustion → lock,
//! clock-skew tampering → lock) is topologically legal — previously these
//! writes happened as ad-hoc raw SQL scattered across `worker.rs`/`state.rs`
//! with no shared enforcement, so an illegal transition (e.g. `LOCKED`
//! silently becoming `ACTIVE` again without a real reactivation) had nothing
//! stopping it.
//!
//! Deliberately **not** used for the "trust a freshly server-verified JWT"
//! writes (a successful `license_activate` or a successful periodic
//! validation) — those establish a wholesale-fresh authoritative state from
//! an externally-verified source, which is a different kind of write than a
//! local graph transition, and must never be blocked by this graph.

use anyhow::{bail, Result};
use rusqlite::Connection;

use super::state::{get_license_state, LicenseStatus};

/// The local transition graph. `AnonymousEval -> Locked` is included even
/// though Document 30's own transition list doesn't name it, to preserve
/// existing clock-skew-lock behavior for a fresh, never-activated install —
/// not introducing a new rejection for a case the docs don't address either
/// way. `Locked` has no outgoing edges in this graph: reactivation goes
/// through `license_activate` establishing fresh state, not a local
/// transition.
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

/// Applies a local status transition, enforcing `is_legal_transition`.
/// A no-op (not an error) if `to` already equals the current status.
/// Errors on any transition not in the legal graph above — most notably
/// `LOCKED -> ACTIVE`, which must never happen without a real reactivation
/// through `license_activate` (a different write path entirely).
pub fn transition(conn: &Connection, to: LicenseStatus) -> Result<()> {
    let current = get_license_state(conn)?;
    let from = current
        .map(|s| s.subscription_status_cached)
        .unwrap_or(LicenseStatus::AnonymousEval);

    if from == to {
        return Ok(());
    }
    if !is_legal_transition(&from, &to) {
        bail!(
            "Illegal license state transition: {:?} -> {:?}",
            from,
            to
        );
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

    /// `test_license_state_machine_transitions` (Document 30 TASK-AUTH-009):
    /// every legal transition succeeds, illegal ones are rejected.
    #[test]
    fn test_license_state_machine_transitions() {
        let conn = crate::db::test_helpers::setup_test_db();

        // Legal transitions, walked in sequence.
        seed_state(&conn, LicenseStatus::AnonymousEval);
        transition(&conn, LicenseStatus::Trial).unwrap();
        assert_eq!(
            get_license_state(&conn).unwrap().unwrap().subscription_status_cached,
            LicenseStatus::Trial
        );

        transition(&conn, LicenseStatus::Active).unwrap();
        assert_eq!(
            get_license_state(&conn).unwrap().unwrap().subscription_status_cached,
            LicenseStatus::Active
        );

        transition(&conn, LicenseStatus::Grace).unwrap();
        assert_eq!(
            get_license_state(&conn).unwrap().unwrap().subscription_status_cached,
            LicenseStatus::Grace
        );

        transition(&conn, LicenseStatus::Active).unwrap();
        assert_eq!(
            get_license_state(&conn).unwrap().unwrap().subscription_status_cached,
            LicenseStatus::Active
        );

        transition(&conn, LicenseStatus::Locked).unwrap();
        assert_eq!(
            get_license_state(&conn).unwrap().unwrap().subscription_status_cached,
            LicenseStatus::Locked
        );

        // Illegal: LOCKED -> ACTIVE without reactivation (Document 30's own
        // named example) must be rejected.
        let result = transition(&conn, LicenseStatus::Active);
        assert!(result.is_err(), "LOCKED -> ACTIVE must be rejected without a real reactivation");

        // Still LOCKED after the rejected attempt.
        assert_eq!(
            get_license_state(&conn).unwrap().unwrap().subscription_status_cached,
            LicenseStatus::Locked
        );
    }

    #[test]
    fn transition_to_the_same_state_is_a_no_op() {
        let conn = crate::db::test_helpers::setup_test_db();
        seed_state(&conn, LicenseStatus::Active);
        transition(&conn, LicenseStatus::Active).unwrap();
        assert_eq!(
            get_license_state(&conn).unwrap().unwrap().subscription_status_cached,
            LicenseStatus::Active
        );
    }

    #[test]
    fn grace_to_locked_is_legal() {
        let conn = crate::db::test_helpers::setup_test_db();
        seed_state(&conn, LicenseStatus::Grace);
        transition(&conn, LicenseStatus::Locked).unwrap();
        assert_eq!(
            get_license_state(&conn).unwrap().unwrap().subscription_status_cached,
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
