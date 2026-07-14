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

pub fn get_account(conn: &Connection, id: &str) -> Result<Option<ConnectedAccountsRow>> {
    let mut stmt = conn.prepare("SELECT * FROM connected_accounts WHERE id = ?1")?;
    let mut rows = stmt.query([id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row_to_account(row)?))
    } else {
        Ok(None)
    }
}

pub fn get_all_accounts(conn: &Connection) -> Result<Vec<ConnectedAccountsRow>> {
    let mut stmt = conn.prepare("SELECT * FROM connected_accounts")?;
    let rows = stmt.query_map([], row_to_account)?;

    let mut accounts = Vec::new();
    for row in rows {
        accounts.push(row?);
    }
    Ok(accounts)
}

pub fn delete_account(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM connected_accounts WHERE id = ?1", params![id])?;
    Ok(())
}

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
