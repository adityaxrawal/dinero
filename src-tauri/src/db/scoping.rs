//! TASK-DB-022: Application-Level Query Scoping (Single-Tenant Isolation).
//!
//! SQLite has no native Row-Level Security. Dinero is single-tenant by
//! construction — `local_profile.id INTEGER PRIMARY KEY CHECK(id = 1)`
//! (TASK-DB-003) means exactly one profile row can ever exist — so every
//! query implicitly operates in the context of that one profile. Document
//! 22 §13.1 requires this to be resolved internally, never accepted as a
//! caller-supplied IPC parameter: a forged or stale `profile_id` argument
//! from a compromised/buggy frontend build must be structurally impossible
//! to act on, not merely rejected by an incidental FK constraint.
//!
//! This is the schema-layer half of the isolation model; TASK-AUTH-008
//! enforces the equivalent at the IPC-middleware layer (Area 3).

/// The one and only `local_profile.id` value that can ever exist. IPC
/// commands must resolve this constant internally rather than accepting a
/// `profile_id` parameter from the caller.
pub const LOCAL_PROFILE_ID: i64 = 1;

#[cfg(test)]
mod tests {
    use super::*;

    /// `test_user_data_isolation` (Document 30 TASK-DB-022): a forged or
    /// stale `profile_id` supplied via a mock IPC call must be ignored,
    /// never trusted. The strongest form of that guarantee is structural:
    /// `auth_google_start` (the one `#[tauri::command]` that used to accept
    /// `profile_id: i64` directly from the frontend) no longer has that
    /// parameter in its signature at all — there is no longer any value for
    /// a compromised or buggy webview build to forge, since
    /// `commands::auth_google_start` resolves `LOCAL_PROFILE_ID` internally.
    /// That's a compile-time property, not something a runtime assertion
    /// can re-derive here, so this test instead pins the one runtime
    /// invariant every other scoped table depends on: the constant this
    /// module hands out must always equal the single row
    /// `local_profile.id INTEGER PRIMARY KEY CHECK(id = 1)` (TASK-DB-003)
    /// can ever contain.
    #[test]
    fn local_profile_id_constant_matches_the_single_allowed_row() {
        assert_eq!(LOCAL_PROFILE_ID, 1);
    }

    /// Exercises the actual DB-level backstop: even if some future call site
    /// reintroduced a caller-supplied `profile_id`, the `CHECK(id = 1)`
    /// constraint (TASK-DB-003) and `connected_accounts.profile_id`'s FK to
    /// `local_profile(id)` mean any value other than `1` is rejected outright
    /// — a forged profile_id can never reference a real row.
    #[test]
    fn forged_profile_id_is_rejected_by_the_db_layer() {
        let conn = crate::db::test_helpers::setup_test_db();
        conn.execute(
            "INSERT INTO local_profile (id) VALUES (1)",
            [],
        )
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
