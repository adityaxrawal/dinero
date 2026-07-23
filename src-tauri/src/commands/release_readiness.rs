//! Doc 30 TASK-OPS-009: Metrics and Release Readiness Dashboard.
//!
//! Backs the Debug page's "Release Readiness" tab
//! (`src/components/debug/ReleaseReadinessViewer.tsx`). Two things, kept
//! deliberately separate:
//!
//! - **Locally-verifiable metrics** (`compute_local_metrics`): real
//!   aggregate queries against this device's own encrypted DB -- see
//!   `ops/release_metrics.sql` for the same queries kept as a standalone,
//!   reviewable reference. Never a substitute for the labeled benchmark
//!   corpus (extraction accuracy, false-positive rate, etc.) -- there is no
//!   ground-truth label in a real user's own data to compare against, so
//!   those are measured by the test suite instead.
//! - **Go/no-go status** (`read_go_no_go`): read from
//!   `scripts/verify_acceptance_criteria.py --output <path>`'s JSON, never
//!   invoked by the app itself (the shipped app has no Python/Cargo/CI
//!   toolchain to run that script with) -- this only ever reflects the
//!   most recent run *someone* (a developer, CI) already performed. Absent
//!   or unparsable output fails closed (`all_passed: false`), per Doc 15
//!   Core Principle 12 -- an unknown test-suite status is never presented
//!   as "go".

use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::State;

/// Doc 30 TASK-OPS-009 acceptance `test_release_dashboard_shows_aggregate_metrics_only`:
/// every field here is a count, rate, or byte size -- never a merchant name,
/// amount, or any other per-transaction/per-user financial detail.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
pub struct LocalMetrics {
    pub unresolved_clusters: i64,
    pub llm_fallback_rate: f64,
    pub db_size_bytes: i64,
    pub statement_parse_failure_rate: f64,
}

/// Same 4 queries as `ops/release_metrics.sql` -- kept in sync manually
/// (that file is a reviewable reference, not a second execution path; see
/// its own header comment).
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

/// Fail closed (Doc 15 Core Principle 12) via `#[derive(Default)]`: `bool`'s
/// default is `false` and `Option`'s is `None`, so the derived default is
/// already "no data is never go" -- `all_passed: false, available: false,
/// checked_at: None` -- with no manual impl needed.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
pub struct GoNoGoStatus {
    pub all_passed: bool,
    /// `false` when no output file was found or it couldn't be parsed --
    /// distinct from `all_passed: false` (a real failing run) so the UI can
    /// show "never checked" rather than implying a run actually failed.
    pub available: bool,
    pub checked_at: Option<String>,
}

/// `CARGO_MANIFEST_DIR` is `src-tauri/`'s own directory, baked in at compile
/// time -- this resolves to the repo root regardless of which crate calls
/// it (a property of the crate being compiled, not of the caller).
fn acceptance_criteria_output_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("release_readiness_check.json")
}

/// Reads the go/no-go status left behind by a prior
/// `python3 scripts/verify_acceptance_criteria.py --output release_readiness_check.json`
/// run. Never runs that script itself -- see this module's doc comment.
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
    GoNoGoStatus { all_passed, available: true, checked_at }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ReleaseReadinessSnapshot {
    pub id: String,
    pub captured_at: String,
    pub metrics: LocalMetrics,
    pub go_no_go: bool,
}

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

pub fn insert_snapshot(conn: &Connection, metrics: &LocalMetrics, go_no_go: bool) -> Result<String> {
    let id = uuid::Uuid::new_v4().to_string();
    let metrics_json = serde_json::to_string(metrics)?;
    conn.execute(
        "INSERT INTO release_readiness_snapshots (id, metrics_json, go_no_go) VALUES (?1, ?2, ?3)",
        rusqlite::params![id, metrics_json, go_no_go as i64],
    )?;
    Ok(id)
}

pub fn list_snapshots(conn: &Connection, limit: i64) -> rusqlite::Result<Vec<ReleaseReadinessSnapshot>> {
    let mut stmt = conn.prepare(
        "SELECT id, captured_at, metrics_json, go_no_go FROM release_readiness_snapshots \
         ORDER BY captured_at DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map(rusqlite::params![limit], row_to_snapshot)?;
    rows.collect()
}

/// Doc 30 TASK-OPS-009 acceptance `test_trend_view_highlights_regressions`:
/// a metric "regresses" release-over-release if it gets *worse* --
/// `db_size_bytes` growing is expected as an install ages and isn't itself
/// a quality signal, so it's excluded here (shown in the UI for context,
/// never flagged as a regression).
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
pub async fn release_readiness_capture_snapshot(
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<ReleaseReadinessSnapshot, crate::error::AppError> {
    let conn = pool.get().await.map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let go_no_go = read_go_no_go().all_passed;
    conn.interact(move |c| -> anyhow::Result<ReleaseReadinessSnapshot> {
        let metrics = compute_local_metrics(c)?;
        let id = insert_snapshot(c, &metrics, go_no_go)?;
        let captured_at: String = c.query_row(
            "SELECT captured_at FROM release_readiness_snapshots WHERE id = ?1",
            rusqlite::params![id.clone()],
            |r| r.get(0),
        )?;
        Ok(ReleaseReadinessSnapshot { id, captured_at, metrics, go_no_go })
    })
    .await
    .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
    .map_err(|e| crate::error::AppError::Db(e.to_string()))
}

#[tauri::command]
pub async fn release_readiness_list_snapshots(
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<Vec<ReleaseReadinessSnapshot>, crate::error::AppError> {
    let conn = pool.get().await.map_err(|e| crate::error::AppError::Db(e.to_string()))?;
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
        // Structural guarantee: the exact field set is 4 named aggregates,
        // nothing else -- no merchant/amount/free-text field could be added
        // here without this test's JSON key assertion catching it.
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

        // No file at all: fails closed, not a false "go".
        let status = read_go_no_go();
        assert!(!status.all_passed);
        assert!(!status.available);

        // A real failing run.
        std::fs::write(&path, r#"{"results": [], "all_passed": false}"#).unwrap();
        let status = read_go_no_go();
        assert!(status.available);
        assert!(!status.all_passed);

        // A real passing run.
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
        // db_size_bytes growing alone must never be flagged.
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
