//! Aggregates local health metrics into a release go/no-go view.
//!
//! Combines figures computed here with the acceptance-criteria results written
//! by the CI gate script, so the in-app readiness panel reflects both runtime
//! health and the risk-register test outcomes.
use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
pub struct LocalMetrics {
    pub unresolved_clusters: i64,
    pub llm_fallback_rate: f64,
    pub db_size_bytes: i64,
    pub statement_parse_failure_rate: f64,
}

/// Computes the local health metrics behind the readiness view.
pub fn compute_local_metrics(conn: &Connection) -> rusqlite::Result<LocalMetrics> {
    let unresolved_clusters: i64 = conn.query_row(
        "SELECT count(*) FROM reconciliation_clusters WHERE cluster_status IN ('open', 'deferred')",
        [],
        |r| r.get(0),
    )?;

    let llm_fallback_rate: Option<f64> = conn.query_row(
        "SELECT CAST(SUM(CASE WHEN extraction_method = 'llm' THEN 1 ELSE 0 END) AS REAL) \
             / NULLIF(COUNT(*), 0) FROM transaction_observations",
        [],
        |r| r.get(0),
    )?;

    let db_size_bytes: i64 = conn.query_row(
        "SELECT page_count * page_size FROM pragma_page_count(), pragma_page_size()",
        [],
        |r| r.get(0),
    )?;

    let statement_parse_failure_rate: Option<f64> = conn.query_row(
        "SELECT CAST(COUNT(*) AS REAL) \
             / NULLIF((SELECT COUNT(*) FROM statements WHERE created_at >= datetime('now', '-30 days')) + COUNT(*), 0) \
         FROM unprocessed_statements \
         WHERE created_at >= datetime('now', '-30 days') AND resolved_statement_id IS NULL",
        [],
        |r| r.get(0),
    )?;

    Ok(LocalMetrics {
        unresolved_clusters,
        llm_fallback_rate: llm_fallback_rate.unwrap_or(0.0),
        db_size_bytes,
        statement_parse_failure_rate: statement_parse_failure_rate.unwrap_or(0.0),
    })
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
pub struct GoNoGoStatus {
    pub all_passed: bool,
    pub available: bool,
    pub checked_at: Option<String>,
}

/// Path of the JSON file the acceptance-criteria gate writes.
fn acceptance_criteria_output_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("release_readiness_check.json")
}

/// Reads the go/no-go status produced by the CI gate.
///
/// Written by the acceptance-criteria script rather than computed here, so the
/// in-app view reflects the same result CI enforced.
pub fn read_go_no_go() -> GoNoGoStatus {
    let path = acceptance_criteria_output_path();
    let Ok(content) = std::fs::read_to_string(&path) else {
        return GoNoGoStatus::default();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
        return GoNoGoStatus::default();
    };
    let Some(all_passed) = value.get("all_passed").and_then(|v| v.as_bool()) else {
        return GoNoGoStatus::default();
    };
    let checked_at = std::fs::metadata(&path)
        .ok()
        .and_then(|m| m.modified().ok())
        .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339());
    GoNoGoStatus {
        all_passed,
        available: true,
        checked_at,
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ReleaseReadinessSnapshot {
    pub id: String,
    pub captured_at: String,
    pub metrics: LocalMetrics,
    pub go_no_go: bool,
}

/// Maps a result row onto a readiness snapshot.
fn row_to_snapshot(row: &rusqlite::Row) -> rusqlite::Result<ReleaseReadinessSnapshot> {
    let metrics_json: String = row.get(2)?;
    let metrics: LocalMetrics = serde_json::from_str(&metrics_json).unwrap_or_default();
    Ok(ReleaseReadinessSnapshot {
        id: row.get(0)?,
        captured_at: row.get(1)?,
        metrics,
        go_no_go: row.get::<_, i64>(3)? != 0,
    })
}

/// Stores a readiness snapshot for later comparison.
pub fn insert_snapshot(
    conn: &Connection,
    metrics: &LocalMetrics,
    go_no_go: bool,
) -> Result<String> {
    let id = uuid::Uuid::new_v4().to_string();
    let metrics_json = serde_json::to_string(metrics)?;
    conn.execute(
        "INSERT INTO release_readiness_snapshots (id, metrics_json, go_no_go) VALUES (?1, ?2, ?3)",
        rusqlite::params![id, metrics_json, go_no_go as i64],
    )?;
    Ok(id)
}

/// Lists stored snapshots, newest first.
pub fn list_snapshots(
    conn: &Connection,
    limit: i64,
) -> rusqlite::Result<Vec<ReleaseReadinessSnapshot>> {
    let mut stmt = conn.prepare(
        "SELECT id, captured_at, metrics_json, go_no_go FROM release_readiness_snapshots \
         ORDER BY captured_at DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map(rusqlite::params![limit], row_to_snapshot)?;
    rows.collect()
}

/// Reports which metrics regressed between two snapshots.
///
/// Comparing against the previous snapshot is what turns absolute numbers into a
/// direction of travel, which is the actually useful signal before a release.
pub fn detect_regressions(previous: &LocalMetrics, current: &LocalMetrics) -> Vec<&'static str> {
    let mut regressions = Vec::new();
    if current.unresolved_clusters > previous.unresolved_clusters {
        regressions.push("unresolved_clusters");
    }
    if current.llm_fallback_rate > previous.llm_fallback_rate {
        regressions.push("llm_fallback_rate");
    }
    if current.statement_parse_failure_rate > previous.statement_parse_failure_rate {
        regressions.push("statement_parse_failure_rate");
    }
    regressions
}

#[tauri::command]
/// Captures a snapshot of current readiness.
pub async fn release_readiness_capture_snapshot(
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<ReleaseReadinessSnapshot, crate::error::AppError> {
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let go_no_go = read_go_no_go().all_passed;
    conn.interact(move |c| -> anyhow::Result<ReleaseReadinessSnapshot> {
        let metrics = compute_local_metrics(c)?;
        let id = insert_snapshot(c, &metrics, go_no_go)?;
        let captured_at: String = c.query_row(
            "SELECT captured_at FROM release_readiness_snapshots WHERE id = ?1",
            rusqlite::params![id.clone()],
            |r| r.get(0),
        )?;
        Ok(ReleaseReadinessSnapshot {
            id,
            captured_at,
            metrics,
            go_no_go,
        })
    })
    .await
    .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
    .map_err(|e| crate::error::AppError::Db(e.to_string()))
}

#[tauri::command]
/// Lists readiness snapshots.
pub async fn release_readiness_list_snapshots(
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<Vec<ReleaseReadinessSnapshot>, crate::error::AppError> {
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(|c| list_snapshots(c, 20))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_release_dashboard_shows_aggregate_metrics_only() {
        let metrics = LocalMetrics::default();
        let value = serde_json::to_value(&metrics).unwrap();
        let obj = value.as_object().unwrap();
        let mut keys: Vec<&str> = obj.keys().map(|s| s.as_str()).collect();
        keys.sort();
        assert_eq!(
            keys,
            vec![
                "db_size_bytes",
                "llm_fallback_rate",
                "statement_parse_failure_rate",
                "unresolved_clusters",
            ]
        );
    }

    #[test]
    fn test_go_no_go_reflects_test_suite_status() {
        let path = acceptance_criteria_output_path();
        let _ = std::fs::remove_file(&path);

        let status = read_go_no_go();
        assert!(!status.all_passed);
        assert!(!status.available);

        std::fs::write(&path, r#"{"results": [], "all_passed": false}"#).unwrap();
        let status = read_go_no_go();
        assert!(status.available);
        assert!(!status.all_passed);

        std::fs::write(&path, r#"{"results": [], "all_passed": true}"#).unwrap();
        let status = read_go_no_go();
        assert!(status.available);
        assert!(status.all_passed);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_trend_view_highlights_regressions() {
        let baseline = LocalMetrics {
            unresolved_clusters: 2,
            llm_fallback_rate: 0.05,
            db_size_bytes: 1_000_000,
            statement_parse_failure_rate: 0.01,
        };

        let worse = LocalMetrics {
            unresolved_clusters: 5,
            llm_fallback_rate: 0.20,
            db_size_bytes: 5_000_000,
            statement_parse_failure_rate: 0.01,
        };
        let regressions = detect_regressions(&baseline, &worse);
        assert!(regressions.contains(&"unresolved_clusters"));
        assert!(regressions.contains(&"llm_fallback_rate"));
        assert!(!regressions.contains(&"statement_parse_failure_rate"));
        assert!(!regressions.contains(&"db_size_bytes"));

        let same_or_better = LocalMetrics {
            unresolved_clusters: 1,
            llm_fallback_rate: 0.05,
            db_size_bytes: 9_000_000,
            statement_parse_failure_rate: 0.0,
        };
        assert!(detect_regressions(&baseline, &same_or_better).is_empty());
    }
}
