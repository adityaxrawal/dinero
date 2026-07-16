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

// Tags

pub fn insert(conn: &Connection, tag: &TagsRow) -> Result<()> {
    conn.execute(
        "INSERT INTO tags (id, name, color_hex, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![tag.id, tag.name, tag.color_hex, tag.created_at],
    )?;
    Ok(())
}

pub fn update(conn: &Connection, tag: &TagsRow) -> Result<()> {
    conn.execute(
        "UPDATE tags SET name = ?2, color_hex = ?3 WHERE id = ?1",
        params![tag.id, tag.name, tag.color_hex],
    )?;
    Ok(())
}

pub fn select_by_id(conn: &Connection, id: &str) -> Result<Option<TagsRow>> {
    let mut stmt = conn.prepare("SELECT * FROM tags WHERE id = ?1")?;
    let mut rows = stmt.query([id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row_to_tag(row)?))
    } else {
        Ok(None)
    }
}

pub fn select_all(conn: &Connection) -> Result<Vec<TagsRow>> {
    let mut stmt = conn.prepare("SELECT * FROM tags ORDER BY name ASC")?;
    let rows = stmt.query_map([], row_to_tag)?;

    let mut tags = Vec::new();
    for row in rows {
        tags.push(row?);
    }
    Ok(tags)
}

/// Doc 30 TASK-API-007: `tags_delete`. Removes any `transaction_tags`
/// join rows first -- `transaction_tags.tag_id REFERENCES tags(id)` with
/// no `ON DELETE CASCADE`, so deleting a still-referenced tag would raise
/// a foreign key violation whenever `PRAGMA foreign_keys` is on.
pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM transaction_tags WHERE tag_id = ?1",
        params![id],
    )?;
    conn.execute("DELETE FROM tags WHERE id = ?1", params![id])?;
    Ok(())
}

fn row_to_tag(row: &Row) -> rusqlite::Result<TagsRow> {
    Ok(TagsRow {
        id: row.get("id")?,
        name: row.get("name")?,
        color_hex: row.get("color_hex")?,
        created_at: row.get("created_at")?,
    })
}

// Transaction Tags

pub fn insert_transaction_tag(conn: &Connection, tx_tag: &TransactionTagsRow) -> Result<()> {
    conn.execute(
        "INSERT INTO transaction_tags (transaction_id, tag_id, created_at) VALUES (?1, ?2, ?3)",
        params![tx_tag.transaction_id, tx_tag.tag_id, tx_tag.created_at],
    )?;
    Ok(())
}

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

pub fn delete_transaction_tag(conn: &Connection, transaction_id: &str, tag_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM transaction_tags WHERE transaction_id = ?1 AND tag_id = ?2",
        params![transaction_id, tag_id],
    )?;
    Ok(())
}

fn row_to_transaction_tag(row: &Row) -> rusqlite::Result<TransactionTagsRow> {
    Ok(TransactionTagsRow {
        transaction_id: row.get("transaction_id")?,
        tag_id: row.get("tag_id")?,
        created_at: row.get("created_at")?,
    })
}
