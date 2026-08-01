use anyhow::Result;
use regex::Regex;

/// Result of a duplicate check.
#[derive(Debug, PartialEq)]
pub enum DuplicateCheckResult {
    /// No duplicate found — safe to proceed.
    NoDuplicate,
    /// Bit-identical file already processed (same SHA-256 hash).
    DuplicateFileHash,
    /// Same instrument + same billing period already imported.
    DuplicateBillingCycle,
    /// Same Gmail message ID already processed.
    DuplicateSourceMessage,
}

/// Checks whether this file hash already exists in the `statements` or `unprocessed_statements`
/// tables for the given account.
///
/// Per Doc 10 §5.2 and §17: Duplicate file detection uses `sha256(file_content)`.
/// This check must happen BEFORE password resolution to avoid wasting processing.
pub async fn check_file_hash_duplicate(
    sha256_hex: &str,
    _instrument_id: Option<&str>,
    pool: &deadpool_sqlite::Pool,
) -> Result<DuplicateCheckResult> {
    tracing::debug!("Checking file hash duplicate for sha256={}", sha256_hex);
    let conn = pool.get().await?;
    let hash = sha256_hex.to_string();
    let is_duplicate = conn
        .interact(move |c| {
            // Check statements table first. `insert_queued()` (Doc 18 §4.7)
            // is the pipeline's one write path into the dedicated `file_hash`
            // column — this used to check `source_message_id` instead, a
            // proxy-hack from before that column existed, which would have
            // silently stopped matching once `insert_queued` started writing
            // the real hash into `file_hash` instead.
            let count: i64 = c.query_row(
                "SELECT COUNT(*) FROM statements WHERE file_hash = ?",
                [&hash],
                |row| row.get(0),
            )?;
            if count > 0 {
                return Ok::<bool, rusqlite::Error>(true);
            }
            // Also check unprocessed_statements via JSON field
            let count: i64 = c.query_row(
                "SELECT COUNT(*) FROM unprocessed_statements WHERE json_extract(statement_source_json, '$.file_hash') = ?",
                [&hash],
                |row| row.get(0),
            )?;
            Ok(count > 0)
        })
        .await
        .map_err(|e| anyhow::anyhow!("DB interact error: {}", e))??;

    if is_duplicate {
        Ok(DuplicateCheckResult::DuplicateFileHash)
    } else {
        Ok(DuplicateCheckResult::NoDuplicate)
    }
}

/// Validates an email-sourced PDF attachment and returns its SHA-256, or
/// `None` if it is not a usable PDF or has already been imported.
///
/// audit_04 #4: both email paths used to set `StatementJob.file_hash` to the
/// Gmail `message_id` as a "proxy". `file_hash` is a *content* hash — the
/// column `check_file_hash_duplicate` above queries — so the proxy meant the
/// same statement forwarded or re-sent produced two unrelated values and was
/// imported twice, and no email-sourced statement could ever match a manual
/// upload of the same file. The message-id dimension it was standing in for is
/// already covered separately by `check_source_message_duplicate`.
///
/// Called before the `statements` row is created and before password
/// resolution, matching both the manual-upload ordering and this module's own
/// "must happen BEFORE password resolution to avoid wasting processing" rule —
/// a PDF we already have should not cost the user a password prompt.
///
/// Fails open on a DB error: a transient failure should not silently drop a
/// statement, and `check_billing_cycle_duplicate` still runs downstream.
pub async fn hash_email_attachment_if_new(
    bytes: &[u8],
    filename: &str,
    msg_id: &str,
    pool: &deadpool_sqlite::Pool,
) -> Option<String> {
    let file_hash = match crate::statements::validator::validate_and_hash(bytes) {
        Ok(h) => h,
        Err(e) => {
            tracing::warn!(
                "Skipping attachment '{}' on msg_id='{}': {}",
                filename,
                msg_id,
                e
            );
            return None;
        }
    };

    match check_file_hash_duplicate(&file_hash, None, pool).await {
        Ok(DuplicateCheckResult::NoDuplicate) => {}
        Ok(_) => {
            tracing::info!(
                "Skipping attachment '{}' on msg_id='{}': statement already imported (sha256={})",
                filename,
                msg_id,
                file_hash
            );
            return None;
        }
        Err(e) => {
            tracing::warn!(
                "Duplicate check failed for attachment '{}' on msg_id='{}' ({}) — proceeding",
                filename,
                msg_id,
                e
            );
        }
    }

    Some(file_hash)
}

/// Checks whether a statement for this instrument and billing period already exists.
///
/// Duplicate := same `instrument_id` AND same `billing_period_start` AND same `billing_period_end`
/// Per Doc 10 §6.1. If duplicate, action = reject silently, log `duplicate_statement_rejected`.
/// Must be run BEFORE row extraction to avoid wasted parsing effort.
pub async fn check_billing_cycle_duplicate(
    instrument_id: &str,
    billing_period_start: &str, // YYYY-MM-DD
    billing_period_end: &str,   // YYYY-MM-DD
    pool: &deadpool_sqlite::Pool,
) -> Result<DuplicateCheckResult> {
    tracing::debug!(
        "Checking billing cycle duplicate: instrument={} period={} → {}",
        instrument_id,
        billing_period_start,
        billing_period_end
    );
    let conn = pool.get().await?;
    let inst_id = instrument_id.to_string();
    let start = billing_period_start.to_string();
    let end = billing_period_end.to_string();
    let count = conn
        .interact(move |c| {
            c.query_row(
                "SELECT COUNT(*) FROM statements
                 WHERE instrument_id = ? AND billing_period_start = ? AND billing_period_end = ?
                 AND is_duplicate = 0",
                rusqlite::params![inst_id, start, end],
                |row| row.get::<_, i64>(0),
            )
        })
        .await
        .map_err(|e| anyhow::anyhow!("DB interact error: {}", e))??;

    if count > 0 {
        Ok(DuplicateCheckResult::DuplicateBillingCycle)
    } else {
        Ok(DuplicateCheckResult::NoDuplicate)
    }
}

/// Checks whether a Gmail message ID has already been ingested.
/// Per Doc 10 §17 acceptance criterion 3.
pub async fn check_source_message_duplicate(
    source_message_id: &str,
    pool: &deadpool_sqlite::Pool,
) -> Result<DuplicateCheckResult> {
    tracing::debug!(
        "Checking source message duplicate for msg_id={}",
        source_message_id
    );
    let conn = pool.get().await?;
    let msg_id = source_message_id.to_string();
    let count = conn
        .interact(move |c| {
            c.query_row(
                "SELECT COUNT(*) FROM statements WHERE source_message_id = ?",
                [&msg_id],
                |row| row.get::<_, i64>(0),
            )
        })
        .await
        .map_err(|e| anyhow::anyhow!("DB interact error: {}", e))??;

    if count > 0 {
        Ok(DuplicateCheckResult::DuplicateSourceMessage)
    } else {
        Ok(DuplicateCheckResult::NoDuplicate)
    }
}

// ── §5.2 Filename-based billing period heuristic ────────────────────────────

/// Extracts a billing period (start, end as YYYY-MM-DD strings) from a PDF filename.
///
/// Strategy (Doc 10 §5.2 / §6.2):
/// 1. Search for numeric YYYY-MM or YYYY/MM patterns.
/// 2. Search for month-name abbreviation + year (e.g. Jan2024, 2024-Jan).
/// 3. If found → infer start = first day of that month, end = last day.
/// 4. If not found → return None so the caller defers to post-metadata extraction.
///
/// Supported filename patterns (case-insensitive):
///   HDFC_Statement_Jan2024.pdf
///   ICICI_Credit_Card_Statement_2024-01.pdf
///   SBI_2024_03_Statement.pdf
///   statement_202403.pdf
///   Axis_Bank_Mar-2024.pdf
pub fn extract_billing_period_from_filename(filename: &str) -> Option<(String, String)> {
    let lower = filename.to_lowercase();
    let name = std::path::Path::new(&lower)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&lower);

    // Pattern 1: YYYY-MM or YYYY_MM or YYYYMM (6-digit compact)
    if let Some((year, month)) = try_numeric_month_year(name) {
        return build_period(year, month);
    }

    // Pattern 2: Month-name abbreviation + 4-digit year (in either order)
    if let Some((year, month)) = try_named_month_year(name) {
        return build_period(year, month);
    }

    None
}

/// Tries patterns like 2024-01, 2024_01, 202401
fn try_numeric_month_year(name: &str) -> Option<(u32, u32)> {
    // YYYY-MM or YYYY_MM
    let re_sep = Regex::new(r"(?:^|[\-_])(\d{4})[\-_](\d{2})(?:[\-_]|$)").ok()?;
    if let Some(caps) = re_sep.captures(name) {
        let year: u32 = caps[1].parse().ok()?;
        let month: u32 = caps[2].parse().ok()?;
        if (1..=12).contains(&month) && year >= 2000 {
            return Some((year, month));
        }
    }
    // MM-YYYY or MM_YYYY
    let re_rev = Regex::new(r"(?:^|[\-_])(\d{2})[\-_](\d{4})(?:[\-_]|$)").ok()?;
    if let Some(caps) = re_rev.captures(name) {
        let month: u32 = caps[1].parse().ok()?;
        let year: u32 = caps[2].parse().ok()?;
        if (1..=12).contains(&month) && year >= 2000 {
            return Some((year, month));
        }
    }
    // Compact YYYYMM (6 digits after a separator or at word boundary)
    let re_compact = Regex::new(r"(?:^|[\-_ ])(\d{4})(\d{2})(?:[\-_ ]|$)").ok()?;
    if let Some(caps) = re_compact.captures(name) {
        let year: u32 = caps[1].parse().ok()?;
        let month: u32 = caps[2].parse().ok()?;
        if (1..=12).contains(&month) && year >= 2000 {
            return Some((year, month));
        }
    }
    None
}

/// Tries patterns like jan2024, 2024jan, jan-2024, 2024-jan
fn try_named_month_year(name: &str) -> Option<(u32, u32)> {
    const MONTHS: [(&str, u32); 12] = [
        ("jan", 1),
        ("feb", 2),
        ("mar", 3),
        ("apr", 4),
        ("may", 5),
        ("jun", 6),
        ("jul", 7),
        ("aug", 8),
        ("sep", 9),
        ("oct", 10),
        ("nov", 11),
        ("dec", 12),
    ];

    for (abbr, month_num) in &MONTHS {
        // month-name followed by year: jan2024 or jan-2024 or jan_2024
        let re_mn_yr = Regex::new(&format!(r"{}[\-_]?(\d{{4}})", abbr)).ok()?;
        if let Some(caps) = re_mn_yr.captures(name) {
            let year: u32 = caps[1].parse().ok()?;
            if year >= 2000 {
                return Some((year, *month_num));
            }
        }
        // year followed by month-name: 2024jan or 2024-jan or 2024_jan
        let re_yr_mn = Regex::new(&format!(r"(\d{{4}})[\-_]?{}", abbr)).ok()?;
        if let Some(caps) = re_yr_mn.captures(name) {
            let year: u32 = caps[1].parse().ok()?;
            if year >= 2000 {
                return Some((year, *month_num));
            }
        }
    }
    None
}

/// Builds (billing_period_start, billing_period_end) as YYYY-MM-DD strings for the given month.
fn build_period(year: u32, month: u32) -> Option<(String, String)> {
    use chrono::NaiveDate;
    let start = NaiveDate::from_ymd_opt(year as i32, month, 1)?;
    // Last day of month: first day of next month minus one day
    let end = if month == 12 {
        NaiveDate::from_ymd_opt(year as i32 + 1, 1, 1)?
    } else {
        NaiveDate::from_ymd_opt(year as i32, month + 1, 1)?
    }
    .pred_opt()?;
    Some((
        start.format("%Y-%m-%d").to_string(),
        end.format("%Y-%m-%d").to_string(),
    ))
}

/// Combined filename-first duplicate cycle check (Doc 10 §5.2).
///
/// Returns:
///   - `DuplicateBillingCycle` if filename yields a clear period AND that period already exists.
///   - `NoDuplicate`  if filename yields a period and it is new.
///   - `None`         if filename yields no parseable period → caller must defer to post-metadata.
pub async fn check_filename_billing_cycle(
    filename: &str,
    instrument_id: &str,
    pool: &deadpool_sqlite::Pool,
) -> Result<Option<DuplicateCheckResult>> {
    match extract_billing_period_from_filename(filename) {
        Some((start, end)) => {
            let result = check_billing_cycle_duplicate(instrument_id, &start, &end, pool).await?;
            Ok(Some(result))
        }
        None => {
            tracing::debug!(
                "Filename '{}' yielded no billing period — deferring to post-metadata check",
                filename
            );
            Ok(None)
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: spin up an in-memory test DB with full schema
    async fn setup_db() -> deadpool_sqlite::Pool {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("test.db");
        crate::db::init_db(db_path).await.unwrap()
    }

    // ── Filename heuristic unit tests ────────────────────────────────────────

    #[test]
    fn filename_numeric_yyyy_mm_hyphen() {
        let result = extract_billing_period_from_filename("ICICI_Credit_2024-01.pdf");
        assert_eq!(
            result,
            Some(("2024-01-01".to_string(), "2024-01-31".to_string()))
        );
    }

    #[test]
    fn filename_numeric_yyyy_mm_underscore() {
        let result = extract_billing_period_from_filename("SBI_2024_03_Statement.pdf");
        assert_eq!(
            result,
            Some(("2024-03-01".to_string(), "2024-03-31".to_string()))
        );
    }

    #[test]
    fn filename_named_month_jan() {
        let result = extract_billing_period_from_filename("HDFC_Statement_Jan2024.pdf");
        assert_eq!(
            result,
            Some(("2024-01-01".to_string(), "2024-01-31".to_string()))
        );
    }

    #[test]
    fn filename_named_month_dec_year_boundary() {
        // December: end should be 2024-12-31
        let result = extract_billing_period_from_filename("axis_dec2024.pdf");
        assert_eq!(
            result,
            Some(("2024-12-01".to_string(), "2024-12-31".to_string()))
        );
    }

    #[test]
    fn filename_named_month_feb_non_leap() {
        let result = extract_billing_period_from_filename("statement_feb2023.pdf");
        assert_eq!(
            result,
            Some(("2023-02-01".to_string(), "2023-02-28".to_string()))
        );
    }

    #[test]
    fn filename_named_month_feb_leap() {
        let result = extract_billing_period_from_filename("kotak_feb2024.pdf");
        assert_eq!(
            result,
            Some(("2024-02-01".to_string(), "2024-02-29".to_string()))
        );
    }

    #[test]
    fn filename_year_then_month_name() {
        let result = extract_billing_period_from_filename("2024-Mar-Statement.pdf");
        assert_eq!(
            result,
            Some(("2024-03-01".to_string(), "2024-03-31".to_string()))
        );
    }

    #[test]
    fn filename_no_date_returns_none() {
        let result = extract_billing_period_from_filename("account-statement.pdf");
        assert_eq!(result, None);
    }

    #[test]
    fn filename_ambiguous_numbers_returns_none() {
        // "statement_12.pdf" — no year, cannot infer
        let result = extract_billing_period_from_filename("statement_12.pdf");
        assert_eq!(result, None);
    }

    // ── test_duplicate_hash_rejected (Doc 30 TASK-STMT-001) ──────────────────

    #[tokio::test]
    async fn test_duplicate_hash_rejected() {
        let pool = setup_db().await;

        let conn = pool.get().await.unwrap();
        conn.interact(|c| {
            crate::db::statements::insert_queued(
                c,
                "stmt_hash_dup",
                "manual_upload",
                None,
                Some("deadbeef00112233"),
            )
            .unwrap();
        })
        .await
        .unwrap();

        let result = check_file_hash_duplicate("deadbeef00112233", None, &pool)
            .await
            .unwrap();
        assert_eq!(result, DuplicateCheckResult::DuplicateFileHash);

        let no_match = check_file_hash_duplicate("0000000000000000", None, &pool)
            .await
            .unwrap();
        assert_eq!(no_match, DuplicateCheckResult::NoDuplicate);
    }

    /// audit_04 #4: email-sourced statements used the Gmail `message_id` as a
    /// `file_hash` "proxy", so the same PDF arriving twice (forwarded, re-sent,
    /// or already manually uploaded) never matched and was imported again.
    /// The hash must be of the bytes, and a known one must be skipped.
    #[tokio::test]
    async fn email_attachment_hash_is_content_derived_and_skips_reimports() {
        let pool = setup_db().await;
        let pdf = b"%PDF-1.4 statement bytes";

        let first = hash_email_attachment_if_new(pdf, "stmt.pdf", "msg_a", &pool)
            .await
            .expect("a new attachment must be accepted");
        assert_ne!(first, "msg_a", "file_hash must not be the message id");
        assert_eq!(
            first,
            crate::statements::validator::validate_and_hash(pdf).unwrap(),
            "must be the same sha256 the manual-upload path computes"
        );

        // Same bytes from a *different* message -- a forward or re-send. The
        // message-id proxy produced a fresh value here and re-imported.
        let same_bytes_other_message =
            hash_email_attachment_if_new(pdf, "stmt.pdf", "msg_b", &pool).await;
        assert_eq!(
            same_bytes_other_message,
            Some(first.clone()),
            "hash depends on content, not on which message carried it"
        );

        // Once imported under that hash, it must be skipped.
        let conn = pool.get().await.unwrap();
        let hash = first.clone();
        conn.interact(move |c| {
            crate::db::statements::insert_queued(
                c,
                "stmt_email_dup",
                "gmail_email",
                Some("msg_a"),
                Some(&hash),
            )
            .unwrap();
        })
        .await
        .unwrap();
        assert_eq!(
            hash_email_attachment_if_new(pdf, "stmt.pdf", "msg_b", &pool).await,
            None,
            "an already-imported statement must not be enqueued again"
        );

        // Not a PDF at all -- skipped before a row or a password prompt.
        assert_eq!(
            hash_email_attachment_if_new(b"not a pdf", "notes.pdf", "msg_c", &pool).await,
            None
        );
    }

    // ── test_filename_billing_period_heuristic (Doc 30 TASK-STMT-001) ───────
    // (the individual `filename_*` unit tests above already exercise every
    // named pattern; this is the one test matching the spec's exact name)

    #[test]
    fn test_filename_billing_period_heuristic() {
        assert_eq!(
            extract_billing_period_from_filename("HDFC_Jan_2026.pdf"),
            Some(("2026-01-01".to_string(), "2026-01-31".to_string()))
        );
    }

    // ── test_ambiguous_filename_defers_check (Doc 30 TASK-STMT-002) ──────────
    // A dedicated test for the deferral behavior alone, distinct from
    // test_duplicate_cycle_skipped_after_metadata_extraction's fuller
    // filename-then-post-metadata round trip.

    #[tokio::test]
    async fn test_ambiguous_filename_defers_check() {
        let pool = setup_db().await;

        let conn = pool.get().await.unwrap();
        conn.interact(|c| {
            c.execute(
                "INSERT INTO instruments (id, type, issuer_name, masked_identifier) \
                 VALUES ('inst_ambig', 'credit_card', 'HDFC', '2222')",
                [],
            )
            .unwrap();
        })
        .await
        .unwrap();

        // "statement.pdf" has no year/month pattern at all — the heuristic
        // must yield None rather than guessing, deferring to Doc 30
        // TASK-STMT-004's real metadata extraction.
        let result = check_filename_billing_cycle("statement.pdf", "inst_ambig", &pool)
            .await
            .unwrap();
        assert_eq!(
            result, None,
            "an ambiguous filename must defer, not silently pick a period"
        );
    }

    // ── test_duplicate_cycle_skipped_from_filename (Doc 10 §5.2) ────────────

    #[tokio::test]
    async fn test_duplicate_cycle_skipped_from_filename() {
        let pool = setup_db().await;

        // Seed: insert instrument + statement for 2024-01 cycle
        let conn = pool.get().await.unwrap();
        conn.interact(|c| {
            c.execute(
                "INSERT INTO instruments (id, type, issuer_name, masked_identifier) \
                 VALUES ('inst_fn', 'credit_card', 'ICICI', '5678')",
                [],
            )
            .unwrap();
            c.execute(
                "INSERT INTO statements \
                 (id, instrument_id, statement_type, billing_period_start, billing_period_end, \
                  parse_status, is_duplicate) \
                 VALUES ('stmt_fn', 'inst_fn', 'credit_card', '2024-01-01', '2024-01-31', \
                         'parsed', 0)",
                [],
            )
            .unwrap();
        })
        .await
        .unwrap();

        // Filename clearly encodes the already-imported billing cycle
        let result = check_filename_billing_cycle("ICICI_Credit_2024-01.pdf", "inst_fn", &pool)
            .await
            .unwrap();

        assert_eq!(result, Some(DuplicateCheckResult::DuplicateBillingCycle));
    }

    // ── test_duplicate_cycle_skipped_after_metadata_extraction (Doc 10 §5.2) ─

    #[tokio::test]
    async fn test_duplicate_cycle_skipped_after_metadata_extraction() {
        let pool = setup_db().await;

        // Seed instrument + statement for 2024-06 cycle
        let conn = pool.get().await.unwrap();
        conn.interact(|c| {
            c.execute(
                "INSERT INTO instruments (id, type, issuer_name, masked_identifier) \
                 VALUES ('inst_meta', 'credit_card', 'HDFC', '9012')",
                [],
            )
            .unwrap();
            c.execute(
                "INSERT INTO statements \
                 (id, instrument_id, statement_type, billing_period_start, billing_period_end, \
                  parse_status, is_duplicate) \
                 VALUES ('stmt_meta', 'inst_meta', 'credit_card', '2024-06-01', '2024-06-30', \
                         'parsed', 0)",
                [],
            )
            .unwrap();
        })
        .await
        .unwrap();

        // Generic filename yields None from heuristic
        let filename_result =
            check_filename_billing_cycle("account-statement.pdf", "inst_meta", &pool)
                .await
                .unwrap();
        assert_eq!(
            filename_result, None,
            "Generic filename should yield None — defer to post-metadata"
        );

        // After metadata extraction provides billing period, check_billing_cycle_duplicate
        // should detect the duplicate
        let post_meta_result =
            check_billing_cycle_duplicate("inst_meta", "2024-06-01", "2024-06-30", &pool)
                .await
                .unwrap();
        assert_eq!(
            post_meta_result,
            DuplicateCheckResult::DuplicateBillingCycle
        );

        // Different period: must pass
        let new_period =
            check_billing_cycle_duplicate("inst_meta", "2024-07-01", "2024-07-31", &pool)
                .await
                .unwrap();
        assert_eq!(new_period, DuplicateCheckResult::NoDuplicate);
    }

    // ── Original test for billing period duplicate ───────────────────────────

    #[tokio::test]
    async fn test_duplicate_billing_period_rejected() {
        let pool = setup_db().await;

        let conn = pool.get().await.unwrap();
        conn.interact(|c| {
            c.execute(
                "INSERT INTO instruments (id, type, issuer_name, masked_identifier) \
                 VALUES ('inst_1', 'credit_card', 'HDFC', '1234')",
                [],
            )
            .unwrap();
            c.execute(
                "INSERT INTO statements \
                 (id, instrument_id, statement_type, billing_period_start, billing_period_end, \
                  parse_status, is_duplicate) \
                 VALUES ('stmt_1', 'inst_1', 'credit_card', '2023-01-01', '2023-01-31', \
                         'parsed', 0)",
                [],
            )
            .unwrap();
        })
        .await
        .unwrap();

        let res = check_billing_cycle_duplicate("inst_1", "2023-01-01", "2023-01-31", &pool)
            .await
            .unwrap();
        assert_eq!(res, DuplicateCheckResult::DuplicateBillingCycle);

        let res2 = check_billing_cycle_duplicate("inst_1", "2023-02-01", "2023-02-28", &pool)
            .await
            .unwrap();
        assert_eq!(res2, DuplicateCheckResult::NoDuplicate);
    }

    // ── test_duplicate_statement_reuploaded_after_deletion_succeeds ────────
    #[tokio::test]
    async fn test_duplicate_statement_reuploaded_after_deletion_succeeds() {
        let temp_dir =
            std::env::temp_dir().join(format!("dinero_dup_del_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let pool = crate::db::init_db(temp_dir.join("test.db")).await.unwrap();

        let conn = pool.get().await.unwrap();
        conn.interact(|c| {
            c.execute(
                "INSERT INTO instruments (id, type, issuer_name, network, masked_identifier, status) 
                 VALUES ('inst_del', 'credit_card', 'HDFC', 'VISA', '1234', 'active')",
                [],
            )
            .unwrap();
            
            // Insert a statement for the billing period
            c.execute(
                "INSERT INTO statements \
                 (id, instrument_id, statement_type, billing_period_start, billing_period_end, \
                  parse_status, is_duplicate) \
                 VALUES ('stmt_del', 'inst_del', 'credit_card', '2023-01-01', '2023-01-31', \
                         'parsed', 0)",
                [],
            )
            .unwrap();
        })
        .await
        .unwrap();

        let res = check_billing_cycle_duplicate("inst_del", "2023-01-01", "2023-01-31", &pool)
            .await
            .unwrap();
        assert_eq!(res, DuplicateCheckResult::DuplicateBillingCycle);

        // Delete the statement simulating a user deleting a rejected duplicate or bad parse
        conn.interact(|c| {
            c.execute("DELETE FROM statements WHERE id = 'stmt_del'", [])
                .unwrap();
        })
        .await
        .unwrap();

        let res_after_del =
            check_billing_cycle_duplicate("inst_del", "2023-01-01", "2023-01-31", &pool)
                .await
                .unwrap();
        // Since the previous one is deleted, it should NOT flag as duplicate, allowing re-upload
        assert_eq!(res_after_del, DuplicateCheckResult::NoDuplicate);
    }
}
