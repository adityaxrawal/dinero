//! Gmail accounts connected for ingestion.
//!
//! Stores account identity and sync bookkeeping only. OAuth tokens live in the
//! OS keychain, never in the database, so a leaked database file cannot be used
//! to read anyone's mail.
use anyhow::Result;
use chrono::NaiveDateTime;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ConnectedAccountsRow {
    pub id: String,
    pub profile_id: i64,
    pub email_address: Option<String>,
    pub account_status: Option<String>,
    pub last_history_id: Option<String>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
}

/// Restores accounts saved before an app-data reset, all-or-nothing.
///
/// Accounts already present are skipped, so a restore that was interrupted
/// can simply run again. Returns how many were inserted.
pub fn restore_from_backup(conn: &Connection, accounts: &[ConnectedAccountsRow]) -> Result<usize> {
    let tx = conn.unchecked_transaction()?;
    let mut inserted = 0;
    for account in accounts {
        if get_account(&tx, &account.id)?.is_none() {
            insert_account(&tx, account)?;
            inserted += 1;
        }
    }
    tx.commit()?;
    Ok(inserted)
}

/// Record a newly connected Gmail account.
pub fn insert_account(conn: &Connection, account: &ConnectedAccountsRow) -> Result<()> {
    conn.execute(
        "INSERT INTO connected_accounts (
            id, profile_id, email_address, account_status, last_history_id, created_at, updated_at
         )
         VALUES (?1, ?2, ?3, ?4, ?5, COALESCE(?6, CURRENT_TIMESTAMP), COALESCE(?7, CURRENT_TIMESTAMP))",
        params![
            account.id,
            account.profile_id,
            account.email_address,
            account.account_status,
            account.last_history_id,
            account.created_at,
            account.updated_at,
        ],
    )?;
    Ok(())
}

/// Update an account's sync bookkeeping.
pub fn update_account(conn: &Connection, account: &ConnectedAccountsRow) -> Result<()> {
    let count = conn.execute(
        "UPDATE connected_accounts SET
            profile_id = ?2,
            email_address = ?3,
            account_status = ?4,
            last_history_id = ?5
         WHERE id = ?1",
        params![
            account.id,
            account.profile_id,
            account.email_address,
            account.account_status,
            account.last_history_id,
        ],
    )?;
    if count == 0 {
        return Err(anyhow::anyhow!("Account not found"));
    }
    Ok(())
}

/// Fetch one account.
pub fn get_account(conn: &Connection, id: &str) -> Result<Option<ConnectedAccountsRow>> {
    let mut stmt = conn.prepare("SELECT * FROM connected_accounts WHERE id = ?1")?;
    let mut rows = stmt.query([id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row_to_account(row)?))
    } else {
        Ok(None)
    }
}

/// All connected accounts.
pub fn get_all_accounts(conn: &Connection) -> Result<Vec<ConnectedAccountsRow>> {
    let mut stmt = conn.prepare("SELECT * FROM connected_accounts")?;
    let rows = stmt.query_map([], row_to_account)?;

    let mut accounts = Vec::new();
    for row in rows {
        accounts.push(row?);
    }
    Ok(accounts)
}

/// Remove an account record.
///
/// The OAuth grant is revoked separately; deleting this row alone would leave a
/// working token behind in the keychain.
pub fn delete_account(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM connected_accounts WHERE id = ?1", params![id])?;
    Ok(())
}

/// Maps a result row onto an account record.
fn row_to_account(row: &Row) -> rusqlite::Result<ConnectedAccountsRow> {
    Ok(ConnectedAccountsRow {
        id: row.get("id")?,
        profile_id: row.get("profile_id")?,
        email_address: row.get("email_address")?,
        account_status: row.get("account_status")?,
        last_history_id: row.get("last_history_id")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

#[cfg(test)]
mod restore_tests {
    use super::*;

    fn acct(id: &str) -> ConnectedAccountsRow {
        ConnectedAccountsRow {
            id: id.into(),
            profile_id: 1,
            email_address: Some(format!("{id}@e.com")),
            account_status: Some("ACTIVE".into()),
            last_history_id: None,
            created_at: None,
            updated_at: None,
        }
    }

    /// A restore inserts missing accounts, skips existing ones (so it can be
    /// retried), and is all-or-nothing.
    #[test]
    fn restore_is_retryable_and_all_or_nothing() {
        let conn = crate::db::test_helpers::setup_test_db();
        conn.execute("INSERT INTO local_profile (id) VALUES (1)", []).unwrap();
        insert_account(&conn, &acct("a")).unwrap();
        assert_eq!(restore_from_backup(&conn, &[acct("a"), acct("b")]).unwrap(), 1);

        conn.execute_batch(
            "CREATE TRIGGER fail_c BEFORE INSERT ON connected_accounts WHEN NEW.id = 'c'
             BEGIN SELECT RAISE(ABORT, 'simulated failure'); END;",
        )
        .unwrap();
        assert!(restore_from_backup(&conn, &[acct("d"), acct("c")]).is_err());
        assert!(get_account(&conn, "d").unwrap().is_none(), "a failed restore inserts nothing");
    }
}
