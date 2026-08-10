//! Tamper-evident audit log.
//!
//! Each row chains to its predecessor by hash, so `verify_chain` can detect
//! whether history was altered after the fact. Detection, not prevention: a
//! writer with database access could rewrite the chain wholesale, but no
//! individual row can be quietly edited without breaking the links after it.
use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The chain's starting hash, used as the predecessor of the first row.
///
/// A fixed all-zero value, so an empty log has a well-defined origin and
/// verification needs no special case for the first entry.
fn genesis_hash() -> String {
    "0".repeat(64)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLogRow {
    pub id: String,
    pub actor_type: Option<String>,
    pub actor_id: Option<String>,
    pub action: Option<String>,
    pub resource_type: Option<String>,
    pub resource_id: Option<String>,
    pub before_json: Option<serde_json::Value>,
    pub after_json: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

/// Computes a row's hash over its content and its predecessor's hash.
///
/// Including the previous hash is what forms the chain: editing any earlier row
/// invalidates every hash after it, so tampering cannot be confined to one entry.
///
/// Every field participates, and absent values hash as an empty string so the
/// input is deterministic regardless of which optional fields are set.
fn compute_row_hash(prev_hash: &str, row: &AuditLogRow) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prev_hash.as_bytes());
    hasher.update(row.id.as_bytes());
    hasher.update(row.actor_type.as_deref().unwrap_or("").as_bytes());
    hasher.update(row.actor_id.as_deref().unwrap_or("").as_bytes());
    hasher.update(row.action.as_deref().unwrap_or("").as_bytes());
    hasher.update(row.resource_type.as_deref().unwrap_or("").as_bytes());
    hasher.update(row.resource_id.as_deref().unwrap_or("").as_bytes());
    hasher.update(
        row.before_json
            .as_ref()
            .map(|v| v.to_string())
            .unwrap_or_default()
            .as_bytes(),
    );
    hasher.update(
        row.after_json
            .as_ref()
            .map(|v| v.to_string())
            .unwrap_or_default()
            .as_bytes(),
    );
    hasher.update(
        row.created_at
            .format("%Y-%m-%d %H:%M:%S")
            .to_string()
            .as_bytes(),
    );
    format!("{:x}", hasher.finalize())
}

/// Recomputes the chain and reports whether it is intact.
///
/// Walks rows in order, deriving each hash from the last. A mismatch means the
/// log was altered after it was written.
pub fn verify_chain(conn: &Connection) -> Result<bool> {
    let mut stmt = conn.prepare(
        "SELECT id, actor_type, actor_id, action, resource_type, resource_id, before_json, after_json, created_at, prev_hash, row_hash
         FROM audit_log ORDER BY rowid ASC",
    )?;
    let rows = stmt.query_map([], |r| {
        let row = AuditLogRow {
            id: r.get(0)?,
            actor_type: r.get(1)?,
            actor_id: r.get(2)?,
            action: r.get(3)?,
            resource_type: r.get(4)?,
            resource_id: r.get(5)?,
            before_json: r.get(6)?,
            after_json: r.get(7)?,
            created_at: r.get(8)?,
        };
        let prev_hash: String = r.get(9)?;
        let row_hash: String = r.get(10)?;
        Ok((row, prev_hash, row_hash))
    })?;

    let mut expected_prev_hash = genesis_hash();
    for entry in rows {
        let (row, prev_hash, row_hash) = entry?;
        if prev_hash != expected_prev_hash {
            return Ok(false);
        }
        if compute_row_hash(&prev_hash, &row) != row_hash {
            return Ok(false);
        }
        expected_prev_hash = row_hash;
    }
    Ok(true)
}

/// Append an entry, linking it to the current chain head.
pub fn insert(conn: &Connection, row: &AuditLogRow) -> Result<()> {
    let prev_hash: String = conn
        .query_row(
            "SELECT row_hash FROM audit_log ORDER BY rowid DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or_else(genesis_hash);
    let row_hash = compute_row_hash(&prev_hash, row);

    conn.execute(
        "INSERT INTO audit_log (
            id, actor_type, actor_id, action, resource_type, resource_id, before_json, after_json, prev_hash, row_hash
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            row.id,
            row.actor_type,
            row.actor_id,
            row.action,
            row.resource_type,
            row.resource_id,
            row.before_json,
            row.after_json,
            prev_hash,
            row_hash,
        ],
    )?;
    Ok(())
}

/// Fetch one audit entry.
pub fn get(conn: &Connection, id: &str) -> Result<Option<AuditLogRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, actor_type, actor_id, action, resource_type, resource_id, before_json, after_json, created_at
         FROM audit_log WHERE id = ?1"
    )?;
    let row = stmt
        .query_row(params![id], |r| {
            Ok(AuditLogRow {
                id: r.get(0)?,
                actor_type: r.get(1)?,
                actor_id: r.get(2)?,
                action: r.get(3)?,
                resource_type: r.get(4)?,
                resource_id: r.get(5)?,
                before_json: r.get(6)?,
                after_json: r.get(7)?,
                created_at: r.get(8)?,
            })
        })
        .optional()?;
    Ok(row)
}

/// All audit entries in chain order.
pub fn fetch_all(
    conn: &Connection,
    resource_type_filter: Option<String>,
    limit: u32,
    offset: u32,
) -> Result<Vec<AuditLogRow>> {
    let mut query = String::from(
        "SELECT id, actor_type, actor_id, action, resource_type, resource_id, before_json, after_json, created_at
         FROM audit_log"
    );
    let mut has_where = false;

    if resource_type_filter.is_some() {
        query.push_str(" WHERE resource_type = ?1");
        has_where = true;
    }

    query.push_str(" ORDER BY created_at DESC LIMIT ?");
    query.push_str(if has_where { "2" } else { "1" });
    query.push_str(" OFFSET ?");
    query.push_str(if has_where { "3" } else { "2" });

    let mut stmt = conn.prepare(&query)?;

    let mut rows = if let Some(rt) = resource_type_filter {
        stmt.query(params![rt, limit, offset])?
    } else {
        stmt.query(params![limit, offset])?
    };

    let mut logs = Vec::new();
    while let Some(row) = rows.next()? {
        logs.push(AuditLogRow {
            id: row.get(0)?,
            actor_type: row.get(1)?,
            actor_id: row.get(2)?,
            action: row.get(3)?,
            resource_type: row.get(4)?,
            resource_id: row.get(5)?,
            before_json: row.get(6)?,
            after_json: row.get(7)?,
            created_at: row.get(8)?,
        });
    }

    Ok(logs)
}
