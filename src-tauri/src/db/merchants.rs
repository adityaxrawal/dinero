use anyhow::Result;
use chrono::NaiveDateTime;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MerchantsRow {
    pub id: String,
    pub name: String,
    pub normalized_name: String,
    pub source: String,
    pub is_deleted: bool,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MerchantAliasesRow {
    pub id: String,
    pub merchant_entity_id: String,
    pub alias_raw: String,
    pub alias_normalized: String,
    pub country_code: Option<String>,
    pub issuer_name: Option<String>,
    pub confidence: f64,
    pub created_at: Option<NaiveDateTime>,
}

pub fn insert(conn: &Connection, merchant: &MerchantsRow) -> Result<()> {
    conn.execute(
        "INSERT INTO merchants (
            id, name, normalized_name, source, is_deleted, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            merchant.id,
            merchant.name,
            merchant.normalized_name,
            merchant.source,
            merchant.is_deleted,
            merchant.created_at,
            merchant.updated_at,
        ],
    )?;
    Ok(())
}

pub fn update(conn: &Connection, merchant: &MerchantsRow) -> Result<()> {
    conn.execute(
        "UPDATE merchants SET
            name = ?2,
            normalized_name = ?3,
            source = ?4,
            is_deleted = ?5,
            updated_at = CURRENT_TIMESTAMP
         WHERE id = ?1",
        params![
            merchant.id,
            merchant.name,
            merchant.normalized_name,
            merchant.source,
            merchant.is_deleted,
        ],
    )?;
    Ok(())
}

pub fn select_by_id(conn: &Connection, id: &str) -> Result<Option<MerchantsRow>> {
    let mut stmt = conn.prepare("SELECT * FROM merchants WHERE id = ?1 AND is_deleted = 0")?;
    let mut rows = stmt.query([id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row_to_merchant(row)?))
    } else {
        Ok(None)
    }
}

/// Looks up a merchant by a raw alias string, normalizing it the same way
/// aliases are stored (uppercased) so callers can pass either raw or
/// already-normalized strings.
pub fn select_by_alias(conn: &Connection, alias: &str) -> Result<Option<MerchantsRow>> {
    let mut stmt = conn.prepare(
        "SELECT m.* FROM merchants m
         JOIN merchant_aliases a ON m.id = a.merchant_entity_id
         WHERE a.alias_normalized = ?1 AND m.is_deleted = 0 LIMIT 1",
    )?;
    let mut rows = stmt.query([alias.to_uppercase()])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row_to_merchant(row)?))
    } else {
        Ok(None)
    }
}

pub fn select_all(conn: &Connection) -> Result<Vec<MerchantsRow>> {
    let mut stmt = conn.prepare("SELECT * FROM merchants WHERE is_deleted = 0")?;
    let rows = stmt.query_map([], row_to_merchant)?;

    let mut merchants = Vec::new();
    for row in rows {
        merchants.push(row?);
    }
    Ok(merchants)
}

pub fn soft_delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "UPDATE merchants SET is_deleted = 1, updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
        params![id],
    )?;
    Ok(())
}

fn row_to_merchant(row: &Row) -> rusqlite::Result<MerchantsRow> {
    Ok(MerchantsRow {
        id: row.get("id")?,
        name: row.get("name")?,
        normalized_name: row.get("normalized_name")?,
        source: row.get("source")?,
        is_deleted: row.get("is_deleted")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

// Aliases

pub fn insert_alias(conn: &Connection, alias: &MerchantAliasesRow) -> Result<()> {
    conn.execute(
        "INSERT INTO merchant_aliases (
            id, merchant_entity_id, alias_raw, alias_normalized, country_code, issuer_name, confidence, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            alias.id,
            alias.merchant_entity_id,
            alias.alias_raw,
            alias.alias_normalized,
            alias.country_code,
            alias.issuer_name,
            alias.confidence,
            alias.created_at,
        ],
    )?;
    Ok(())
}

pub fn select_aliases_by_merchant_id(
    conn: &Connection,
    merchant_entity_id: &str,
) -> Result<Vec<MerchantAliasesRow>> {
    let mut stmt = conn.prepare("SELECT * FROM merchant_aliases WHERE merchant_entity_id = ?1")?;
    let rows = stmt.query_map([merchant_entity_id], row_to_alias)?;

    let mut aliases = Vec::new();
    for row in rows {
        aliases.push(row?);
    }
    Ok(aliases)
}

pub fn delete_alias(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM merchant_aliases WHERE id = ?1", params![id])?;
    Ok(())
}

fn row_to_alias(row: &Row) -> rusqlite::Result<MerchantAliasesRow> {
    Ok(MerchantAliasesRow {
        id: row.get("id")?,
        merchant_entity_id: row.get("merchant_entity_id")?,
        alias_raw: row.get("alias_raw")?,
        alias_normalized: row.get("alias_normalized")?,
        country_code: row.get("country_code")?,
        issuer_name: row.get("issuer_name")?,
        confidence: row.get("confidence")?,
        created_at: row.get("created_at")?,
    })
}
