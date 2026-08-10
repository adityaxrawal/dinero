//! Prevents the same statement being imported twice.
//!
//! Four independent checks, because one signal is never enough: the file hash
//! catches a byte-identical re-upload, the billing cycle catches the same
//! statement re-downloaded and thus differing in bytes, the source message id
//! catches re-ingesting the same email, and the filename period catches a
//! re-issued copy. A duplicate slipping through would double every transaction
//! in the statement.
use anyhow::Result;
use regex::Regex;

#[derive(Debug, PartialEq)]
pub enum DuplicateCheckResult {
    NoDuplicate,
    DuplicateFileHash,
    DuplicateBillingCycle,
    DuplicateSourceMessage,
}

/// Detects a byte-identical re-upload by content hash.
///
/// The strictest and cheapest check, and the only one that catches an exact
/// duplicate regardless of naming.
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
            let count: i64 = c.query_row(
                "SELECT COUNT(*) FROM statements WHERE file_hash = ?",
                [&hash],
                |row| row.get(0),
            )?;
            if count > 0 {
                return Ok::<bool, rusqlite::Error>(true);
            }
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

/// Hashes an email attachment, skipping work if already seen.
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

/// Detects a statement covering an already-imported billing period.
///
/// Catches the case the hash cannot: the same statement re-downloaded is a
/// different file byte-for-byte, but importing it twice would double every
/// transaction it contains.
pub async fn check_billing_cycle_duplicate(
    instrument_id: &str,
    billing_period_start: &str,
    billing_period_end: &str,
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

/// Detects re-ingestion of the same source email.
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

/// Extracts a billing period from a statement filename.
///
/// Filenames frequently encode the period, which is often the only period signal
/// available before the document has been parsed.
pub fn extract_billing_period_from_filename(filename: &str) -> Option<(String, String)> {
    let lower = filename.to_lowercase();
    let name = std::path::Path::new(&lower)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&lower);

    if let Some((year, month)) = try_numeric_month_year(name) {
        return build_period(year, month);
    }

    if let Some((year, month)) = try_named_month_year(name) {
        return build_period(year, month);
    }

    None
}

/// Parses a numeric month/year pair from a filename.
fn try_numeric_month_year(name: &str) -> Option<(u32, u32)> {
    let re_sep = Regex::new(r"(?:^|[\-_])(\d{4})[\-_](\d{2})(?:[\-_]|$)").ok()?;
    if let Some(caps) = re_sep.captures(name) {
        let year: u32 = caps[1].parse().ok()?;
        let month: u32 = caps[2].parse().ok()?;
        if (1..=12).contains(&month) && year >= 2000 {
            return Some((year, month));
        }
    }
    let re_rev = Regex::new(r"(?:^|[\-_])(\d{2})[\-_](\d{4})(?:[\-_]|$)").ok()?;
    if let Some(caps) = re_rev.captures(name) {
        let month: u32 = caps[1].parse().ok()?;
        let year: u32 = caps[2].parse().ok()?;
        if (1..=12).contains(&month) && year >= 2000 {
            return Some((year, month));
        }
    }
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

/// Parses a named month and year from a filename.
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
        let re_mn_yr = Regex::new(&format!(r"{}[\-_]?(\d{{4}})", abbr)).ok()?;
        if let Some(caps) = re_mn_yr.captures(name) {
            let year: u32 = caps[1].parse().ok()?;
            if year >= 2000 {
                return Some((year, *month_num));
            }
        }
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

/// Builds period start and end dates from a month and year.
fn build_period(year: u32, month: u32) -> Option<(String, String)> {
    use chrono::NaiveDate;
    let start = NaiveDate::from_ymd_opt(year as i32, month, 1)?;
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

/// Checks a filename-derived period against already-imported statements.
///
/// The cheapest of the period checks, since it needs no parsing of the document
/// itself.
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

#[cfg(test)]
mod tests {
    use super::*;

    async fn setup_db() -> deadpool_sqlite::Pool {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("test.db");
        crate::db::init_db(db_path).await.unwrap()
    }

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
        let result = extract_billing_period_from_filename("statement_12.pdf");
        assert_eq!(result, None);
    }

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

        let same_bytes_other_message =
            hash_email_attachment_if_new(pdf, "stmt.pdf", "msg_b", &pool).await;
        assert_eq!(
            same_bytes_other_message,
            Some(first.clone()),
            "hash depends on content, not on which message carried it"
        );

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

        assert_eq!(
            hash_email_attachment_if_new(b"not a pdf", "notes.pdf", "msg_c", &pool).await,
            None
        );
    }

    #[test]
    fn test_filename_billing_period_heuristic() {
        assert_eq!(
            extract_billing_period_from_filename("HDFC_Jan_2026.pdf"),
            Some(("2026-01-01".to_string(), "2026-01-31".to_string()))
        );
    }

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

        let result = check_filename_billing_cycle("statement.pdf", "inst_ambig", &pool)
            .await
            .unwrap();
        assert_eq!(
            result, None,
            "an ambiguous filename must defer, not silently pick a period"
        );
    }

    #[tokio::test]
    async fn test_duplicate_cycle_skipped_from_filename() {
        let pool = setup_db().await;

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

        let result = check_filename_billing_cycle("ICICI_Credit_2024-01.pdf", "inst_fn", &pool)
            .await
            .unwrap();

        assert_eq!(result, Some(DuplicateCheckResult::DuplicateBillingCycle));
    }

    #[tokio::test]
    async fn test_duplicate_cycle_skipped_after_metadata_extraction() {
        let pool = setup_db().await;

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

        let filename_result =
            check_filename_billing_cycle("account-statement.pdf", "inst_meta", &pool)
                .await
                .unwrap();
        assert_eq!(
            filename_result, None,
            "Generic filename should yield None — defer to post-metadata"
        );

        let post_meta_result =
            check_billing_cycle_duplicate("inst_meta", "2024-06-01", "2024-06-30", &pool)
                .await
                .unwrap();
        assert_eq!(
            post_meta_result,
            DuplicateCheckResult::DuplicateBillingCycle
        );

        let new_period =
            check_billing_cycle_duplicate("inst_meta", "2024-07-01", "2024-07-31", &pool)
                .await
                .unwrap();
        assert_eq!(new_period, DuplicateCheckResult::NoDuplicate);
    }

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
        assert_eq!(res_after_del, DuplicateCheckResult::NoDuplicate);
    }
}
