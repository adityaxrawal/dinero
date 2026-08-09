use chrono::NaiveDateTime;
use rusqlite::{params, Connection, Result, Row};

#[derive(Debug, Clone, PartialEq)]
pub struct StatementDraftRow {
    pub id: String,
    pub origin: String,
    pub file_hash: String,
    pub instrument_id: Option<String>,
    pub issuer_name: Option<String>,
    pub masked_identifier: Option<String>,
    pub instrument_type: Option<String>,
    pub billing_period_start: Option<String>,
    pub billing_period_end: Option<String>,
    pub due_date: Option<String>,
    pub statement_date: Option<String>,
    pub current_balance: Option<i64>,
    pub minimum_due: Option<i64>,
    pub rows_json: String,
    pub status: String,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
}

const SELECT_COLUMNS: &str =
    "id, origin, file_hash, instrument_id, issuer_name, masked_identifier, \
     instrument_type, billing_period_start, billing_period_end, due_date, statement_date, \
     current_balance, minimum_due, rows_json, status, created_at, updated_at";

fn row_from_sql(row: &Row) -> rusqlite::Result<StatementDraftRow> {
    Ok(StatementDraftRow {
        id: row.get(0)?,
        origin: row.get(1)?,
        file_hash: row.get(2)?,
        instrument_id: row.get(3)?,
        issuer_name: row.get(4)?,
        masked_identifier: row.get(5)?,
        instrument_type: row.get(6)?,
        billing_period_start: row.get(7)?,
        billing_period_end: row.get(8)?,
        due_date: row.get(9)?,
        statement_date: row.get(10)?,
        current_balance: row.get(11)?,
        minimum_due: row.get(12)?,
        rows_json: row.get(13)?,
        status: row.get(14)?,
        created_at: row.get(15)?,
        updated_at: row.get(16)?,
    })
}

pub fn insert(conn: &Connection, draft: &StatementDraftRow) -> Result<()> {
    conn.execute(
        "INSERT INTO statement_drafts \
         (id, origin, file_hash, instrument_id, issuer_name, masked_identifier, instrument_type, \
          billing_period_start, billing_period_end, due_date, statement_date, current_balance, \
          minimum_due, rows_json, status) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            draft.id,
            draft.origin,
            draft.file_hash,
            draft.instrument_id,
            draft.issuer_name,
            draft.masked_identifier,
            draft.instrument_type,
            draft.billing_period_start,
            draft.billing_period_end,
            draft.due_date,
            draft.statement_date,
            draft.current_balance,
            draft.minimum_due,
            draft.rows_json,
            draft.status,
        ],
    )?;
    Ok(())
}

pub fn select_by_id(conn: &Connection, id: &str) -> Result<Option<StatementDraftRow>> {
    conn.query_row(
        &format!("SELECT {SELECT_COLUMNS} FROM statement_drafts WHERE id = ?1"),
        params![id],
        row_from_sql,
    )
    .map(Some)
    .or_else(|e| {
        if e == rusqlite::Error::QueryReturnedNoRows {
            Ok(None)
        } else {
            Err(e)
        }
    })
}

pub fn select_pending_review(conn: &Connection) -> Result<Vec<StatementDraftRow>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SELECT_COLUMNS} FROM statement_drafts WHERE status = 'pending_review' ORDER BY created_at DESC"
    ))?;
    let rows = stmt.query_map([], row_from_sql)?;
    rows.collect()
}

pub fn update_status(conn: &Connection, id: &str, status: &str) -> Result<()> {
    conn.execute(
        "UPDATE statement_drafts SET status = ?1 WHERE id = ?2",
        params![status, id],
    )?;
    Ok(())
}

pub fn delete(conn: &Connection, id: &str) -> Result<bool> {
    let count = conn.execute("DELETE FROM statement_drafts WHERE id = ?1", params![id])?;
    Ok(count > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_row() -> StatementDraftRow {
        StatementDraftRow {
            id: "draft_1".to_string(),
            origin: "manual_upload".to_string(),
            file_hash: "abc123".to_string(),
            instrument_id: Some("inst_1".to_string()),
            issuer_name: Some("HDFC".to_string()),
            masked_identifier: Some("1111".to_string()),
            instrument_type: Some("credit_card".to_string()),
            billing_period_start: Some("2026-06-01".to_string()),
            billing_period_end: Some("2026-06-30".to_string()),
            due_date: Some("2026-07-15".to_string()),
            statement_date: None,
            current_balance: Some(500_000),
            minimum_due: Some(25_000),
            rows_json: "[]".to_string(),
            status: "pending_review".to_string(),
            created_at: None,
            updated_at: None,
        }
    }

    #[tokio::test]
    async fn test_insert_and_select_by_id_round_trips() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let pool = crate::db::init_db(temp_dir.join("test.db")).await.unwrap();
        let conn = pool.get().await.unwrap();
        conn.interact(|c| {
            c.execute(
                "INSERT INTO instruments (id, type, issuer_name, masked_identifier) \
                 VALUES ('inst_1', 'credit_card', 'HDFC', '1111')",
                [],
            )
        })
        .await
        .unwrap()
        .unwrap();

        let row = sample_row();
        conn.interact({
            let row = row.clone();
            move |c| insert(c, &row)
        })
        .await
        .unwrap()
        .unwrap();

        let fetched = conn
            .interact(|c| select_by_id(c, "draft_1"))
            .await
            .unwrap()
            .unwrap()
            .expect("draft must exist");
        assert_eq!(fetched.issuer_name.as_deref(), Some("HDFC"));
        assert_eq!(fetched.status, "pending_review");
    }

    #[tokio::test]
    async fn test_select_pending_review_excludes_committed_and_discarded() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let pool = crate::db::init_db(temp_dir.join("test.db")).await.unwrap();
        let conn = pool.get().await.unwrap();
        conn.interact(|c| {
            c.execute(
                "INSERT INTO instruments (id, type, issuer_name, masked_identifier) \
                 VALUES ('inst_1', 'credit_card', 'HDFC', '1111')",
                [],
            )
        })
        .await
        .unwrap()
        .unwrap();

        let pending = sample_row();
        let mut committed = sample_row();
        committed.id = "draft_2".to_string();
        committed.status = "committed".to_string();
        conn.interact(move |c| {
            insert(c, &pending).unwrap();
            insert(c, &committed).unwrap();
        })
        .await
        .unwrap();

        let rows = conn
            .interact(|c| select_pending_review(c))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "draft_1");
    }

    #[tokio::test]
    async fn test_update_status_and_delete() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let pool = crate::db::init_db(temp_dir.join("test.db")).await.unwrap();
        let conn = pool.get().await.unwrap();
        conn.interact(|c| {
            c.execute(
                "INSERT INTO instruments (id, type, issuer_name, masked_identifier) \
                 VALUES ('inst_1', 'credit_card', 'HDFC', '1111')",
                [],
            )
        })
        .await
        .unwrap()
        .unwrap();
        let row = sample_row();
        conn.interact({
            let row = row.clone();
            move |c| insert(c, &row)
        })
        .await
        .unwrap()
        .unwrap();

        conn.interact(|c| update_status(c, "draft_1", "discarded"))
            .await
            .unwrap()
            .unwrap();
        let fetched = conn
            .interact(|c| select_by_id(c, "draft_1"))
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, "discarded");

        let deleted = conn
            .interact(|c| delete(c, "draft_1"))
            .await
            .unwrap()
            .unwrap();
        assert!(deleted);
        let gone = conn
            .interact(|c| select_by_id(c, "draft_1"))
            .await
            .unwrap()
            .unwrap();
        assert!(gone.is_none());
    }
}
