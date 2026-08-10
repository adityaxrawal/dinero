//! User-defined tags and their transaction associations.
//!
//! A join table gives the many-to-many relationship, so a tag can be renamed
//! once and every transaction carrying it follows automatically.
use anyhow::Result;
use chrono::NaiveDateTime;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct TagsRow {
    pub id: String,
    pub name: String,
    pub color_hex: Option<String>,
    pub created_at: Option<NaiveDateTime>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct TransactionTagsRow {
    pub transaction_id: String,
    pub tag_id: String,
    pub created_at: Option<NaiveDateTime>,
}

/// Create a tag.
pub fn insert(conn: &Connection, tag: &TagsRow) -> Result<()> {
    conn.execute(
        "INSERT INTO tags (id, name, color_hex, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![tag.id, tag.name, tag.color_hex, tag.created_at],
    )?;
    Ok(())
}

/// Rename a tag.
///
/// Renaming in place means every transaction carrying it follows automatically,
/// which is the reason tags are referenced by id rather than by name.
pub fn update(conn: &Connection, tag: &TagsRow) -> Result<()> {
    conn.execute(
        "UPDATE tags SET name = ?2, color_hex = ?3 WHERE id = ?1",
        params![tag.id, tag.name, tag.color_hex],
    )?;
    Ok(())
}

/// Fetch one tag.
pub fn select_by_id(conn: &Connection, id: &str) -> Result<Option<TagsRow>> {
    let mut stmt = conn.prepare("SELECT * FROM tags WHERE id = ?1")?;
    let mut rows = stmt.query([id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row_to_tag(row)?))
    } else {
        Ok(None)
    }
}

/// All tags, for pickers and filters.
pub fn select_all(conn: &Connection) -> Result<Vec<TagsRow>> {
    let mut stmt = conn.prepare("SELECT * FROM tags ORDER BY name ASC")?;
    let rows = stmt.query_map([], row_to_tag)?;

    let mut tags = Vec::new();
    for row in rows {
        tags.push(row?);
    }
    Ok(tags)
}

/// Delete a tag and its transaction associations.
pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM transaction_tags WHERE tag_id = ?1",
        params![id],
    )?;
    conn.execute("DELETE FROM tags WHERE id = ?1", params![id])?;
    Ok(())
}

/// Maps a result row onto a tag record.
fn row_to_tag(row: &Row) -> rusqlite::Result<TagsRow> {
    Ok(TagsRow {
        id: row.get("id")?,
        name: row.get("name")?,
        color_hex: row.get("color_hex")?,
        created_at: row.get("created_at")?,
    })
}

/// Attach a tag to a transaction.
pub fn insert_transaction_tag(conn: &Connection, tx_tag: &TransactionTagsRow) -> Result<()> {
    conn.execute(
        "INSERT INTO transaction_tags (transaction_id, tag_id, created_at) VALUES (?1, ?2, ?3)",
        params![tx_tag.transaction_id, tx_tag.tag_id, tx_tag.created_at],
    )?;
    Ok(())
}

/// Tags attached to one transaction.
pub fn select_by_transaction_id(
    conn: &Connection,
    transaction_id: &str,
) -> Result<Vec<TransactionTagsRow>> {
    let mut stmt = conn.prepare("SELECT * FROM transaction_tags WHERE transaction_id = ?1")?;
    let rows = stmt.query_map([transaction_id], row_to_transaction_tag)?;

    let mut tags = Vec::new();
    for row in rows {
        tags.push(row?);
    }
    Ok(tags)
}

/// Detach a tag from a transaction, leaving the tag itself intact.
pub fn delete_transaction_tag(conn: &Connection, transaction_id: &str, tag_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM transaction_tags WHERE transaction_id = ?1 AND tag_id = ?2",
        params![transaction_id, tag_id],
    )?;
    Ok(())
}

/// Maps a result row onto a transaction-tag association.
fn row_to_transaction_tag(row: &Row) -> rusqlite::Result<TransactionTagsRow> {
    Ok(TransactionTagsRow {
        transaction_id: row.get("transaction_id")?,
        tag_id: row.get("tag_id")?,
        created_at: row.get("created_at")?,
    })
}
