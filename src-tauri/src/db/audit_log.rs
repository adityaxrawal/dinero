use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Genesis value for the first row's `prev_hash` (Document 18 §4.21):
/// `'0'.repeat(64)`.
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

/// Document 18 §4.21: `SHA256(prev_hash || id || actor_type || actor_id ||
/// action || resource_type || resource_id || before_json || after_json ||
/// created_at)`, computed at write time.
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
    // SQLite's DATETIME storage truncates to whole-second precision, so the
    // hash must be computed over that same truncated representation --
    // hashing the full-precision in-memory value would never reproduce the
    // same digest once the row is read back from the DB.
    hasher.update(
        row.created_at
            .format("%Y-%m-%d %H:%M:%S")
            .to_string()
            .as_bytes(),
    );
    format!("{:x}", hasher.finalize())
}

/// Verifies the full `prev_hash`/`row_hash` chain in insertion order,
/// detecting whether any row has been edited or deleted out-of-band after
/// being written (Document 18 §4.21's tamper-evidence design) — a guarantee
/// the immutability trigger alone can't provide, since that trigger only
/// guards the IPC/rusqlite mutation path, not direct SQLite file access.
/// Returns `Ok(true)` if the chain is intact, `Ok(false)` on the first break
/// found (a row's stored `row_hash` doesn't match recomputation, or its
/// `prev_hash` doesn't match the preceding row's `row_hash`).
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

/// Records a consent event (Doc 25 §4.2/§4.4, Doc 06 §7): e.g. Gmail
/// authorization, onboarding disclosures, or a support-bundle export. Doc 18
/// §4.21 (the authoritative schema) specifies these live as `audit_log`
/// entries with `resource_type = 'consent'`, not in a separate
/// `consent_events` table — an earlier BRD draft's naming this document does
/// not follow. `detail` becomes part of `after_json` alongside a UTC
/// timestamp, matching the doc's example format
/// ("User consents to Gmail access on <timestamp>").
pub fn record_consent_event(conn: &Connection, consent_type: &str, detail: &str) -> Result<()> {
    let now = Utc::now();
    let row = AuditLogRow {
        id: uuid::Uuid::new_v4().to_string(),
        actor_type: Some("user".to_string()),
        actor_id: Some("local".to_string()),
        action: Some(format!("consent_{}", consent_type)),
        resource_type: Some("consent".to_string()),
        resource_id: None,
        before_json: None,
        after_json: Some(serde_json::json!({
            "consent_type": consent_type,
            "detail": detail,
            "timestamp": now.to_rfc3339(),
        })),
        created_at: now,
    };
    insert(conn, &row)
}

/// Fetches consent-event history (Doc 25 §4.4: "Consent History" viewer at
/// Settings → Privacy → Consent History) — a thin filter over `fetch_all`
/// scoped to `resource_type = 'consent'`, distinct from the general Network
/// Activity Log which shows traffic, not consent.
pub fn fetch_consent_history(
    conn: &Connection,
    limit: u32,
    offset: u32,
) -> Result<Vec<AuditLogRow>> {
    fetch_all(conn, Some("consent".to_string()), limit, offset)
}

/// Computes and stores the `prev_hash`/`row_hash` tamper-evidence chain
/// (Document 18 §4.21) — always computed here, never accepted from the
/// caller, since `AuditLogRow` itself has no hash fields for a caller to
/// tamper with in the first place.
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
