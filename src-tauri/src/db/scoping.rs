//! Row-scoping constant for the local profile.
//!
//! The schema carries a profile column throughout so multi-profile support
//! remains possible, but this build is single-user and every row is written
//! against one fixed id. Centralising it here keeps that assumption in a single
//! place rather than scattering a literal `1` across every query.

pub const LOCAL_PROFILE_ID: i64 = 1;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_profile_id_constant_matches_the_single_allowed_row() {
        assert_eq!(LOCAL_PROFILE_ID, 1);
    }

    #[test]
    fn forged_profile_id_is_rejected_by_the_db_layer() {
        let conn = crate::db::test_helpers::setup_test_db();
        conn.execute("INSERT INTO local_profile (id) VALUES (1)", [])
            .unwrap();

        let forged_insert = conn.execute(
            "INSERT INTO connected_accounts (id, profile_id, email_address) VALUES ('acc_forged', 999, 'attacker@example.com')",
            [],
        );
        assert!(
            forged_insert.is_err(),
            "a connected_accounts row referencing a forged, nonexistent profile_id must be rejected"
        );

        let legit_insert = conn.execute(
            "INSERT INTO connected_accounts (id, profile_id, email_address) VALUES ('acc_real', 1, 'user@example.com')",
            [],
        );
        assert!(legit_insert.is_ok());
    }
}
