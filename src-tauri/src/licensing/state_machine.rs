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

/// The entitlement a verified server response grants.
///
/// The signed `state` claim wins, so a tampered response body cannot upgrade
/// itself; tokens minted before the claim existed fall back to the response's
/// `state`. With neither, the result is Locked (fail closed). Review C4/H2.
pub fn server_status(
    claims: &crate::licensing::jwt::LicenseClaims,
    response_state: Option<&str>,
) -> LicenseStatus {
    claims
        .state
        .as_deref()
        .or(response_state)
        .map(LicenseStatus::from_server_state)
        .unwrap_or(LicenseStatus::Locked)
}

/// Records a verified server validation: token, status, expiry, timestamp.
///
/// The one place a server answer is written. A verified server response is
/// authoritative, so it may move the licence to any state -- including out of
/// Locked after a renewal -- unlike the local, time-driven transitions above.
/// Errors are returned rather than dropped: a failed write here once meant the
/// token was never refreshed and paying users were locked out after 48h.
pub fn apply_server_validation(
    conn: &Connection,
    status: &LicenseStatus,
    jwt: &str,
    expires_at: chrono::DateTime<chrono::Utc>,
) -> Result<()> {
    let n = conn.execute(
        "UPDATE license_state SET license_jwt = ?1, subscription_status_cached = ?2,
             jwt_expires_at = ?3, current_period_end_cached = ?3,
             last_server_validated_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP
         WHERE id = 1",
        rusqlite::params![jwt, status.as_str(), expires_at],
    )?;
    if n != 1 {
        bail!("no license_state row to update");
    }
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

    fn claims(state: Option<&str>) -> crate::licensing::jwt::LicenseClaims {
        crate::licensing::jwt::LicenseClaims {
            sub: "s".into(),
            device_id: "d".into(),
            plan: "p".into(),
            billing_interval: "monthly".into(),
            exp: 0,
            state: state.map(String::from),
        }
    }

    /// Review C4/H2: the signed claim decides; the body is only a fallback for
    /// old tokens; nothing at all fails closed. "validated" (the old `status`
    /// value) is never an entitlement.
    #[test]
    fn server_status_prefers_signed_claim_and_fails_closed() {
        assert_eq!(server_status(&claims(Some("LOCKED")), Some("ACTIVE")), LicenseStatus::Locked);
        assert_eq!(server_status(&claims(Some("ACTIVE")), None), LicenseStatus::Active);
        assert_eq!(server_status(&claims(None), Some("PAST_DUE")), LicenseStatus::PastDue);
        assert_eq!(server_status(&claims(None), Some("TRIAL")), LicenseStatus::Trial);
        assert_eq!(server_status(&claims(None), None), LicenseStatus::Locked);
        assert_eq!(server_status(&claims(Some("validated")), None), LicenseStatus::Locked);
    }

    /// Review C4: a successful validation is stored (the old raw write of
    /// status "validated" violated the CHECK constraint and was dropped), and
    /// a renewal can lift a Locked licence.
    #[test]
    fn apply_server_validation_stores_token_and_can_unlock() {
        let conn = crate::db::test_helpers::setup_test_db();
        seed_state(&conn, LicenseStatus::Locked);
        let exp = Utc::now() + chrono::Duration::days(7);
        apply_server_validation(&conn, &LicenseStatus::Active, "new-jwt", exp).unwrap();
        let st = get_license_state(&conn).unwrap().unwrap();
        assert_eq!(st.subscription_status_cached, LicenseStatus::Active);
        assert_eq!(st.license_jwt, "new-jwt");
        assert_eq!(st.jwt_expires_at.timestamp(), exp.timestamp());

        apply_server_validation(&conn, &LicenseStatus::Locked, "locked-jwt", exp).unwrap();
        assert_eq!(
            get_license_state(&conn).unwrap().unwrap().subscription_status_cached,
            LicenseStatus::Locked
        );
    }

    #[test]
    fn only_entitling_statuses_permit_writes() {
        use LicenseStatus::*;
        for s in [Active, Grace, PastDue, Trial] {
            assert!(s.is_entitled(), "{s:?}");
        }
        for s in [Locked, Canceled, Expired, AnonymousEval] {
            assert!(!s.is_entitled(), "{s:?}");
        }
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
