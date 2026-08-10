//! Spending category taxonomy.
//!
//! Categories are soft-deleted rather than removed: historical transactions
//! reference them, and a hard delete would orphan those rows and silently
//! rewrite past spending breakdowns.
use anyhow::Result;
use chrono::NaiveDateTime;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct CategoriesRow {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub source_type: String,
    pub mcc_code: Option<String>,
    pub monthly_budget_minor: Option<i64>,
    pub is_deleted: bool,
    pub created_at: Option<NaiveDateTime>,
    pub color: Option<String>,
    pub icon: Option<String>,
}

/// Insert a spending category.
pub fn insert(conn: &Connection, category: &CategoriesRow) -> Result<()> {
    conn.execute(
        "INSERT INTO categories (
            id, parent_id, name, source_type, mcc_code, monthly_budget_minor, is_deleted, created_at, color, icon
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            category.id,
            category.parent_id,
            category.name,
            category.source_type,
            category.mcc_code,
            category.monthly_budget_minor,
            category.is_deleted,
            category.created_at,
            category.color,
            category.icon,
        ],
    )?;
    Ok(())
}

/// Rename a category or change its budget.
pub fn update(conn: &Connection, category: &CategoriesRow) -> Result<()> {
    let existing =
        select_by_id(conn, &category.id)?.ok_or_else(|| anyhow::anyhow!("Category not found"))?;

    if existing.source_type == "system"
        && (existing.name != category.name || existing.parent_id != category.parent_id)
    {
        return Err(anyhow::anyhow!("Cannot rename or move a system category"));
    }

    conn.execute(
        "UPDATE categories SET
            parent_id = ?2,
            name = ?3,
            source_type = ?4,
            mcc_code = ?5,
            monthly_budget_minor = ?6,
            is_deleted = ?7,
            color = ?8,
            icon = ?9
         WHERE id = ?1",
        params![
            category.id,
            category.parent_id,
            category.name,
            category.source_type,
            category.mcc_code,
            category.monthly_budget_minor,
            category.is_deleted,
            category.color,
            category.icon,
        ],
    )?;
    Ok(())
}

/// Fetch one category.
pub fn select_by_id(conn: &Connection, id: &str) -> Result<Option<CategoriesRow>> {
    let mut stmt = conn.prepare("SELECT * FROM categories WHERE id = ?1 AND is_deleted = 0")?;
    let mut rows = stmt.query([id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row_to_category(row)?))
    } else {
        Ok(None)
    }
}

/// All live categories.
pub fn select_all(conn: &Connection) -> Result<Vec<CategoriesRow>> {
    let mut stmt =
        conn.prepare("SELECT * FROM categories WHERE is_deleted = 0 ORDER BY name ASC")?;
    let rows = stmt.query_map([], row_to_category)?;

    let mut categories = Vec::new();
    for row in rows {
        categories.push(row?);
    }
    Ok(categories)
}

/// Soft-delete a category.
///
/// Historical transactions reference it, so hard deletion would orphan them and
/// retroactively rewrite past spending breakdowns.
pub fn soft_delete(conn: &Connection, id: &str, confirm_reassign: bool) -> Result<usize> {
    let cat = select_by_id(conn, id)?.ok_or_else(|| anyhow::anyhow!("Category not found"))?;

    if cat.source_type == "system" {
        return Err(anyhow::anyhow!("Cannot delete a system category"));
    }

    let linked_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM transactions WHERE category_id = ?1 AND is_deleted = 0",
        params![id],
        |row| row.get(0),
    )?;

    if linked_count > 0 {
        if !confirm_reassign {
            anyhow::bail!(
                "{} transactions are linked to this category; pass confirm_reassign=true to reassign them to 'Others' before deleting",
                linked_count
            );
        }
        conn.execute(
            "UPDATE transactions SET category_id = 'cat_others' WHERE category_id = ?1",
            params![id],
        )?;
    }

    conn.execute(
        "UPDATE categories SET is_deleted = 1 WHERE id = ?1",
        params![id],
    )?;
    Ok(linked_count as usize)
}

/// Maps a result row onto a category record.
fn row_to_category(row: &Row) -> rusqlite::Result<CategoriesRow> {
    Ok(CategoriesRow {
        id: row.get("id")?,
        parent_id: row.get("parent_id")?,
        name: row.get("name")?,
        source_type: row.get("source_type")?,
        mcc_code: row.get("mcc_code")?,
        monthly_budget_minor: row.get("monthly_budget_minor")?,
        is_deleted: row.get("is_deleted")?,
        created_at: row.get("created_at")?,
        color: row.get("color")?,
        icon: row.get("icon")?,
    })
}
