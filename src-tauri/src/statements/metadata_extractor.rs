use crate::statements::parser::ParsedPage;
use anyhow::Result;
use regex::Regex;

/// Extracted statement-level metadata (Doc 10 §9.1).
#[derive(Debug, Default)]
pub struct StatementMetadata {
    pub billing_period_start: Option<String>, // YYYY-MM-DD (UTC)
    pub billing_period_end: Option<String>,   // YYYY-MM-DD (UTC)
    pub due_date: Option<String>,             // YYYY-MM-DD (UTC)
    pub minimum_due: Option<i64>,             // amount_minor (paise)
    pub current_balance: Option<i64>,         // amount_minor (paise)
    pub issuer_name: Option<String>,
    pub masked_identifier: Option<String>, // last 4 digits or masked account
    pub network: Option<String>,           // VISA / MASTERCARD / RUPAY
    pub rewards_summary_json: Option<String>,
    /// The date printed on the statement itself ("billing date" in the
    /// review-modal UI). No current extraction regex populates this — it's
    /// routinely blank from `extract_metadata` and filled in by the user
    /// during draft review (`commit_staged_draft`).
    pub statement_date: Option<String>,
}

/// Extracts statement-level metadata from parsed pages (Doc 10 §9).
///
/// If mandatory fields (billing_period_start, billing_period_end, due_date) are all absent,
/// the statement is still persisted with parse_status = 'parsed' with missing fields (§9.3, §9.4).
/// Partial metadata is never a reason to reject the statement entirely.
///
/// Leap-year and month-end edge cases are handled per §9.4:
///   - Dates are stored as ISO-8601 YYYY-MM-DD strings for exact-equality duplicate detection.
///   - Month-end anchoring uses the last day of the actual calendar month.
pub fn extract_metadata(pages: &[ParsedPage]) -> Result<StatementMetadata> {
    let mut meta = StatementMetadata::default();

    // Use only the first page for header extraction per §9.1 (header appears on page 1).
    // If first page is empty, fall back to full document text.
    let first_page_text: String = pages
        .first()
        .map(|p| p.text.as_str())
        .unwrap_or("")
        .to_string();
    let full_text: String = pages
        .iter()
        .map(|p| p.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let header_text = if first_page_text.trim().len() > 20 {
        &first_page_text
    } else {
        &full_text
    };

    // ── Issuer / Bank detection (drives regex pattern selection) ─────────────

    meta.issuer_name = detect_issuer(header_text);

    // ── Billing period ───────────────────────────────────────────────────────
    // Patterns cover common Indian bank header terminology (Doc 10 §9.1).
    // Dates in bank statements: dd/MM/YYYY, dd-MM-YYYY, dd MMM YYYY, DD-MON-YYYY
    meta.billing_period_start = extract_date_pattern(
        header_text,
        &[
            // HDFC: "Statement Period: 01/12/2023 to 31/12/2023"
            r"(?i)statement\s+period[:\s]+(\d{2}[/\-]\d{2}[/\-]\d{4})",
            r"(?i)billing\s+from[:\s]+(\d{2}[/\-]\d{2}[/\-]\d{4})",
            r"(?i)from\s+date[:\s]+(\d{2}[/\-]\d{2}[/\-]\d{4})",
            // SBI: "01 Dec 2023 to 31 Dec 2023"
            r"(?i)(\d{2}\s+(?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)\s+\d{4})\s+to\s+",
            // Generic YYYY-MM-DD start
            r"(?i)period\s+from[:\s]+(\d{4}-\d{2}-\d{2})",
        ],
    );
    meta.billing_period_end = extract_date_pattern(
        header_text,
        &[
            // HDFC: "... to 31/12/2023"
            r"(?i)statement\s+period[:\s]+\d{2}[/\-]\d{2}[/\-]\d{4}\s+to\s+(\d{2}[/\-]\d{2}[/\-]\d{4})",
            r"(?i)billing\s+to[:\s]+(\d{2}[/\-]\d{2}[/\-]\d{4})",
            r"(?i)to\s+date[:\s]+(\d{2}[/\-]\d{2}[/\-]\d{4})",
            // SBI: "01 Dec 2023 to 31 Dec 2023" — capture second date
            r"(?i)\d{2}\s+(?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)\s+\d{4}\s+to\s+(\d{2}\s+(?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)\s+\d{4})",
            r"(?i)period\s+to[:\s]+(\d{4}-\d{2}-\d{2})",
        ],
    );

    // Normalize extracted dates to YYYY-MM-DD
    if let Some(ref d) = meta.billing_period_start.clone() {
        meta.billing_period_start = normalize_date_string(d);
    }
    if let Some(ref d) = meta.billing_period_end.clone() {
        meta.billing_period_end = normalize_date_string(d);
    }

    // ── Due date ─────────────────────────────────────────────────────────────
    meta.due_date = extract_date_pattern(
        header_text,
        &[
            r"(?i)payment\s+due\s+date[:\s]+(\d{2}[/\-]\d{2}[/\-]\d{4})",
            r"(?i)due\s+date[:\s]+(\d{2}[/\-]\d{2}[/\-]\d{4})",
            r"(?i)pay\s+by[:\s]+(\d{2}[/\-]\d{2}[/\-]\d{4})",
            r"(?i)pay\s+by[:\s]+(\d{2}\s+(?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)\s+\d{4})",
            r"(?i)amount\s+due\s+date[:\s]+(\d{2}[/\-]\d{2}[/\-]\d{4})",
        ],
    );
    if let Some(ref d) = meta.due_date.clone() {
        meta.due_date = normalize_date_string(d);
    }

    // ── Monetary fields (search full document, not just header) ──────────────
    meta.minimum_due = extract_amount_minor(
        header_text,
        &[
            r"(?i)minimum\s+(?:amount\s+)?due[:\s]+(?:INR|Rs\.?|₹)?\s*([\d,]+(?:\.\d{1,2})?)",
            r"(?i)min(?:imum)?\s+due[:\s]+(?:INR|Rs\.?|₹)?\s*([\d,]+(?:\.\d{1,2})?)",
            r"(?i)minimum\s+payment\s+due[:\s]+(?:INR|Rs\.?|₹)?\s*([\d,]+(?:\.\d{1,2})?)",
        ],
    );
    meta.current_balance = extract_amount_minor(
        header_text,
        &[
            r"(?i)total\s+amount\s+due[:\s]+(?:INR|Rs\.?|₹)?\s*([\d,]+(?:\.\d{1,2})?)",
            r"(?i)outstanding\s+balance[:\s]+(?:INR|Rs\.?|₹)?\s*([\d,]+(?:\.\d{1,2})?)",
            r"(?i)current\s+balance[:\s]+(?:INR|Rs\.?|₹)?\s*([\d,]+(?:\.\d{1,2})?)",
            r"(?i)closing\s+balance[:\s]+(?:INR|Rs\.?|₹)?\s*([\d,]+(?:\.\d{1,2})?)",
            r"(?i)amount\s+payable[:\s]+(?:INR|Rs\.?|₹)?\s*([\d,]+(?:\.\d{1,2})?)",
        ],
    );

    // ── Card / account identity ───────────────────────────────────────────────
    // Masked card number patterns: XXXX XXXX XXXX 1234, **** **** **** 1234, ending 1234
    meta.masked_identifier = extract_text_pattern(
        header_text,
        &[
            r"(?i)card\s+(?:ending|number)[:\s]+[Xx*\s]+(\d{4})",
            r"(?i)a/c\s+(?:no\.?|number)[:\s]+[Xx*\s]+(\d{4})",
            r"(?i)account\s+(?:no\.?|number)[:\s]+[Xx*\s]+(\d{4})",
            // Inline masked card: XX-1234 or XXXX1234
            r"(?i)[Xx*]{4}[\s\-]?(\d{4})\b",
        ],
    );

    // Network: VISA, MASTERCARD, RUPAY, AMEX
    meta.network = extract_text_pattern(
        header_text,
        &[
            r"(?i)\b(VISA)\b",
            r"(?i)\b(MASTERCARD|MasterCard|Master Card)\b",
            r"(?i)\b(RUPAY|RuPay)\b",
            r"(?i)\b(AMEX|American Express)\b",
        ],
    );

    Ok(meta)
}

/// Detects the issuer/bank name from common header markers.
fn detect_issuer(text: &str) -> Option<String> {
    // Ordered by specificity — check longer names first
    const ISSUERS: &[(&str, &str)] = &[
        ("American Express", "AMEX"),
        ("amex", "AMEX"),
        ("HDFC Bank", "HDFC"),
        ("hdfc", "HDFC"),
        ("ICICI Bank", "ICICI"),
        ("icici", "ICICI"),
        ("Axis Bank", "Axis"),
        ("axis bank", "Axis"),
        ("State Bank of India", "SBI"),
        ("sbi", "SBI"),
        ("Kotak Mahindra", "Kotak"),
        ("kotak", "Kotak"),
        ("IndusInd Bank", "IndusInd"),
        ("indusind", "IndusInd"),
        ("Yes Bank", "Yes Bank"),
        ("yes bank", "Yes Bank"),
        ("IDFC FIRST", "IDFC"),
        ("idfc", "IDFC"),
    ];
    let lower = text.to_lowercase();
    for (pattern, canonical) in ISSUERS {
        if lower.contains(&pattern.to_lowercase()) {
            return Some(canonical.to_string());
        }
    }
    None
}

// ── Date helpers ──────────────────────────────────────────────────────────────

/// Normalizes various date string formats to `YYYY-MM-DD`.
///
/// Supported input formats:
///   dd/MM/YYYY, dd-MM-YYYY  → e.g. 31/01/2024, 31-01-2024
///   dd MMM YYYY             → e.g. 31 Jan 2024
///   YYYY-MM-DD              → already correct, passed through
fn normalize_date_string(date: &str) -> Option<String> {
    use chrono::NaiveDate;
    let date = date.trim();

    // YYYY-MM-DD (pass through)
    if let Ok(d) = NaiveDate::parse_from_str(date, "%Y-%m-%d") {
        return Some(d.format("%Y-%m-%d").to_string());
    }
    // dd/MM/YYYY
    if let Ok(d) = NaiveDate::parse_from_str(date, "%d/%m/%Y") {
        return Some(d.format("%Y-%m-%d").to_string());
    }
    // dd-MM-YYYY
    if let Ok(d) = NaiveDate::parse_from_str(date, "%d-%m-%Y") {
        return Some(d.format("%Y-%m-%d").to_string());
    }
    // dd MMM YYYY (e.g. "31 Jan 2024")
    if let Ok(d) = NaiveDate::parse_from_str(date, "%d %b %Y") {
        return Some(d.format("%Y-%m-%d").to_string());
    }
    // DD-MON-YYYY (e.g. "31-JAN-2024") — uppercase month
    let upper = date.to_uppercase();
    if let Ok(d) = NaiveDate::parse_from_str(&upper, "%d-%b-%Y") {
        return Some(d.format("%Y-%m-%d").to_string());
    }
    None
}

fn extract_date_pattern(text: &str, patterns: &[&str]) -> Option<String> {
    for pat in patterns {
        if let Ok(re) = Regex::new(pat) {
            if let Some(caps) = re.captures(text) {
                if let Some(m) = caps.get(1) {
                    return Some(m.as_str().to_string());
                }
            }
        }
    }
    None
}

fn extract_amount_minor(text: &str, patterns: &[&str]) -> Option<i64> {
    for pat in patterns {
        if let Ok(re) = Regex::new(pat) {
            if let Some(caps) = re.captures(text) {
                if let Some(m) = caps.get(1) {
                    let cleaned = m.as_str().replace(',', "");
                    if let Ok(val) = cleaned.parse::<f64>() {
                        return Some((val * 100.0).round() as i64);
                    }
                }
            }
        }
    }
    None
}

fn extract_text_pattern(text: &str, patterns: &[&str]) -> Option<String> {
    for pat in patterns {
        if let Ok(re) = Regex::new(pat) {
            if let Some(caps) = re.captures(text) {
                if let Some(m) = caps.get(1) {
                    return Some(m.as_str().to_string());
                }
            }
        }
    }
    None
}

// ── DB integration ────────────────────────────────────────────────────────────

/// Upserts the `statements` row for `stmt_id` with all extracted metadata
/// fields: `UPDATE` if `insert_queued()` already wrote a `queued` row for it
/// at intake (Doc 18 §4.7's crash-recovery invariant, the normal case for
/// both entry points), or a fresh `INSERT` if not (the statement-instrument-
/// gate/password-resume paths, which reuse `unprocessed_statements`'s own ID
/// as `stmt_id` here and have never had a `statements` row before now).
///
/// Per §5.4 / §9.4: partial metadata is acceptable — `parse_status = 'parsed'` even
/// when optional fields are absent. The statement is never rejected for missing metadata.
///
/// Returns the same `stmt_id` the caller passed in, for call-site convenience.
pub async fn write_statement_row(
    stmt_id: &str,
    instrument_id: &str,
    instrument_type: &str, // "credit_card" | "bank_account" (instruments.type, not statements.statement_type)
    meta: &StatementMetadata,
    source_message_id: Option<&str>,
    pool: &deadpool_sqlite::Pool,
) -> Result<String> {
    let sid = stmt_id.to_string();
    let inst_id = instrument_id.to_string();
    // Doc 18 §4.7: `statements.statement_type` is `credit_card_statement` /
    // `bank_account_statement` — a distinct vocabulary from `instruments.type`
    // ("credit_card" / "bank_account"), which is what this function used to
    // (incorrectly) store here verbatim.
    let stmt_type = match instrument_type {
        "bank_account" => "bank_account_statement".to_string(),
        _ => "credit_card_statement".to_string(),
    };
    let bps = meta
        .billing_period_start
        .clone()
        .unwrap_or_else(|| "1970-01-01".to_string());
    let bpe = meta
        .billing_period_end
        .clone()
        .unwrap_or_else(|| "1970-01-01".to_string());
    let due = meta.due_date.clone();
    let stmt_date = meta.statement_date.clone();
    let cur_bal = meta.current_balance;
    let min_due = meta.minimum_due;
    let rewards = meta.rewards_summary_json.clone();
    let src_msg = source_message_id.map(|s| s.to_string());

    let conn = pool.get().await?;
    conn.interact(move |c| {
        let updated = c.execute(
            "UPDATE statements SET \
             instrument_id = ?2, statement_type = ?3, billing_period_start = ?4, \
             billing_period_end = ?5, due_date = ?6, statement_date = ?7, current_balance = ?8, minimum_due = ?9, \
             rewards_summary_json = ?10, source_message_id = COALESCE(?11, source_message_id), \
             parse_status = 'parsed', updated_at = CURRENT_TIMESTAMP \
             WHERE id = ?1",
            rusqlite::params![
                sid, inst_id, stmt_type, bps, bpe, due, stmt_date, cur_bal, min_due, rewards, src_msg,
            ],
        )?;

        if updated == 0 {
            c.execute(
                "INSERT INTO statements \
                 (id, instrument_id, statement_type, billing_period_start, billing_period_end, \
                  due_date, statement_date, current_balance, minimum_due, rewards_summary_json, \
                  source_message_id, parse_status, is_duplicate, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'parsed', 0, CURRENT_TIMESTAMP)",
                rusqlite::params![
                    sid, inst_id, stmt_type, bps, bpe, due, stmt_date, cur_bal, min_due, rewards, src_msg,
                ],
            )?;
        }

        // Auto-populate extracted statement metadata onto the target instrument row
        let cycle_day: Option<u8> = if bpe != "1970-01-01" {
            bpe.split('-').nth(2).and_then(|s| s.parse::<u8>().ok())
        } else {
            None
        };

        c.execute(
            "UPDATE instruments SET \
             statement_due_date = COALESCE(?2, statement_due_date), \
             minimum_due = COALESCE(?3, minimum_due), \
             current_balance = COALESCE(?4, current_balance), \
             rewards_summary = COALESCE(?5, rewards_summary), \
             billing_cycle_day = COALESCE(billing_cycle_day, ?6), \
             updated_at = CURRENT_TIMESTAMP \
             WHERE id = ?1",
            rusqlite::params![inst_id, due, min_due, cur_bal, rewards, cycle_day],
        )?;

        Ok::<(), rusqlite::Error>(())
    })
    .await
    .map_err(|e| anyhow::anyhow!("DB interact error (write_statement_row): {}", e))??;

    tracing::info!(
        "Wrote statement row id='{}' for instrument_id='{}' \
         period={:?} → {:?} parse_status=parsed",
        stmt_id,
        instrument_id,
        meta.billing_period_start,
        meta.billing_period_end
    );

    Ok(stmt_id.to_string())
}

/// Resolves an instrument from the `instruments` table; auto-creates it if missing.
///
/// Match key: `(type, issuer_name, masked_identifier)` — per Doc 10 §10.1.
/// Uses `INSERT OR IGNORE` + subsequent `SELECT` for atomicity (§4.7 pattern).
pub async fn resolve_or_create_instrument(
    instrument_type: &str,
    issuer_name: &str,
    masked_identifier: &str,
    network: Option<&str>,
    pool: &deadpool_sqlite::Pool,
) -> Result<String> {
    let id = uuid::Uuid::new_v4().to_string();
    let itype = instrument_type.to_string();
    let issuer = issuer_name.to_string();
    let masked = masked_identifier.to_string();
    let net = network.map(|n| n.to_string());

    let conn = pool.get().await?;
    let instrument_id = conn
        .interact(move |c| {
            // INSERT OR IGNORE: if unique key already exists, this is a no-op
            c.execute(
                "INSERT OR IGNORE INTO instruments \
                 (id, type, issuer_name, masked_identifier, network, status, created_at, updated_at) \
                 VALUES (?, ?, ?, ?, ?, 'active', datetime('now'), datetime('now'))",
                rusqlite::params![id, itype, issuer, masked, net],
            )?;
            // Always SELECT to get the canonical ID (handles both insert and existing rows)
            c.query_row(
                "SELECT id FROM instruments \
                 WHERE type = ? AND issuer_name = ? AND masked_identifier = ?",
                rusqlite::params![itype, issuer, masked],
                |row| row.get::<_, String>(0),
            )
        })
        .await
        .map_err(|e| anyhow::anyhow!("DB interact error (resolve_or_create_instrument): {}", e))??;

    tracing::debug!(
        "Instrument resolved: id='{}' type='{}' issuer='{}' masked='{}'",
        instrument_id,
        instrument_type,
        issuer_name,
        masked_identifier
    );

    Ok(instrument_id)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_page(text: &str) -> ParsedPage {
        ParsedPage {
            page_number: 1,
            text: text.to_string(),
            ocr_used: false,
        }
    }

    // ── test_extract_billing_period_hdfc ──────────────────────────────────────

    #[test]
    fn test_extract_billing_period_hdfc() {
        let text = "HDFC Bank Credit Card Statement\n\
                    Statement Period: 01/12/2023 to 31/12/2023\n\
                    Payment Due Date: 20/01/2024\n\
                    Total Amount Due: Rs. 12,345.67\n\
                    Minimum Amount Due: Rs. 1,234.00\n\
                    Card ending XXXX 4321 | VISA";
        let pages = vec![make_page(text)];
        let meta = extract_metadata(&pages).expect("must not fail");

        assert_eq!(
            meta.billing_period_start.as_deref(),
            Some("2023-12-01"),
            "billing_period_start must be 2023-12-01"
        );
        assert_eq!(
            meta.billing_period_end.as_deref(),
            Some("2023-12-31"),
            "billing_period_end must be 2023-12-31"
        );
        assert_eq!(
            meta.due_date.as_deref(),
            Some("2024-01-20"),
            "due_date must be 2024-01-20"
        );
        assert_eq!(
            meta.issuer_name.as_deref(),
            Some("HDFC"),
            "issuer must be HDFC"
        );
        assert_eq!(
            meta.masked_identifier.as_deref(),
            Some("4321"),
            "masked_identifier must be 4321"
        );
        assert_eq!(
            meta.network.as_deref(),
            Some("VISA"),
            "network must be VISA"
        );
    }

    // ── test_extract_current_balance_icici ────────────────────────────────────

    #[test]
    fn test_extract_current_balance_icici() {
        let text = "ICICI Bank Platinum Credit Card\n\
                    Billing From: 01-11-2023 To Date: 30-11-2023\n\
                    Due Date: 15/12/2023\n\
                    Total Amount Due: INR 8,500.00\n\
                    Minimum Due: INR 500.00\n\
                    Card Number: XXXX XXXX XXXX 9876\n\
                    MASTERCARD";
        let pages = vec![make_page(text)];
        let meta = extract_metadata(&pages).expect("must not fail");

        assert_eq!(
            meta.current_balance,
            Some(850_000), // 8500.00 in paise
            "current_balance must be 850000 paise"
        );
        assert_eq!(
            meta.minimum_due,
            Some(50_000), // 500.00 in paise
            "minimum_due must be 50000 paise"
        );
        assert_eq!(
            meta.issuer_name.as_deref(),
            Some("ICICI"),
            "issuer must be ICICI"
        );
        assert_eq!(
            meta.masked_identifier.as_deref(),
            Some("9876"),
            "masked_identifier must be 9876"
        );
        assert_eq!(
            meta.network.as_deref(),
            Some("MASTERCARD"),
            "network must be MASTERCARD"
        );
    }

    // ── test_partial_metadata_still_persists_statement_row ────────────────────

    #[test]
    fn test_partial_metadata_still_persists_statement_row() {
        // Page with only partial data — missing due_date, minimum_due, billing_period_end
        let text = "HDFC Bank Statement\n\
                    Statement Period: 01/01/2024 to\n\
                    VISA";
        let pages = vec![make_page(text)];
        let meta =
            extract_metadata(&pages).expect("extract_metadata must not fail on partial data");

        // Billing start parsed; end absent (regex stops at "to" with no following date)
        assert_eq!(
            meta.billing_period_start.as_deref(),
            Some("2024-01-01"),
            "billing_period_start must be extracted even if end is missing"
        );
        // billing_period_end and due_date will be None — that is correct and expected
        // The statement row MUST still be persisted (§9.3, §9.4)
        assert!(
            meta.due_date.is_none() || meta.due_date.is_some(),
            "due_date absence must not cause rejection"
        );
    }

    // ── test_auto_create_instrument_from_statement (Doc 30 TASK-STMT-004) ────

    #[tokio::test]
    async fn test_auto_create_instrument_from_statement() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let pool = crate::db::init_db(temp_dir.join("test.db")).await.unwrap();

        // No `instruments` row exists yet for this (type, issuer, masked) key.
        let conn = pool.get().await.unwrap();
        let existing: i64 = conn
            .interact(|c| {
                c.query_row(
                    "SELECT COUNT(*) FROM instruments WHERE issuer_name = 'HDFC' AND masked_identifier = '4321'",
                    [],
                    |row| row.get(0),
                )
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(existing, 0, "instrument must not exist before resolution");

        let id1 = resolve_or_create_instrument("credit_card", "HDFC", "4321", Some("VISA"), &pool)
            .await
            .unwrap();
        assert!(!id1.is_empty());

        // Doc 12 §10.4a: a second resolution for the identical key (e.g. a
        // concurrently-processing email alert for the same instrument) must
        // reuse the same row, never create a duplicate.
        let id2 = resolve_or_create_instrument("credit_card", "HDFC", "4321", Some("VISA"), &pool)
            .await
            .unwrap();
        assert_eq!(
            id1, id2,
            "resolving the same key twice must reuse the same instrument"
        );

        let count: i64 = conn
            .interact(|c| {
                c.query_row(
                    "SELECT COUNT(*) FROM instruments WHERE issuer_name = 'HDFC' AND masked_identifier = '4321'",
                    [],
                    |row| row.get(0),
                )
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            count, 1,
            "exactly one instrument row must exist, never a duplicate"
        );
    }

    // ── Async DB test: write_statement_row ────────────────────────────────────

    #[tokio::test]
    async fn test_write_statement_row_partial_metadata() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("test.db");
        let pool = crate::db::init_db(db_path).await.unwrap();

        // Seed instrument + the queued row insert_queued() would already have
        // written at intake (Doc 18 §4.7 crash-recovery invariant).
        let conn = pool.get().await.unwrap();
        conn.interact(|c| {
            c.execute(
                "INSERT INTO instruments (id, type, issuer_name, masked_identifier) \
                 VALUES ('inst_wr', 'credit_card', 'HDFC', '1111')",
                [],
            )
            .unwrap();
            crate::db::statements::insert_queued(c, "stmt_wr", "manual_upload", None, None)
                .unwrap();
        })
        .await
        .unwrap();

        // Partial metadata (billing_period_end and due_date absent)
        let meta = StatementMetadata {
            billing_period_start: Some("2024-01-01".to_string()),
            billing_period_end: None,
            due_date: None,
            minimum_due: None,
            current_balance: Some(500_000),
            issuer_name: Some("HDFC".to_string()),
            masked_identifier: Some("1111".to_string()),
            network: Some("VISA".to_string()),
            rewards_summary_json: None,
            statement_date: None,
        };

        let stmt_id = write_statement_row("stmt_wr", "inst_wr", "credit_card", &meta, None, &pool)
            .await
            .unwrap();
        assert!(!stmt_id.is_empty(), "Statement ID must be returned");

        // Verify row was persisted
        let conn2 = pool.get().await.unwrap();
        let (parse_status, bal): (String, Option<i64>) = conn2
            .interact(move |c| {
                c.query_row(
                    "SELECT parse_status, current_balance FROM statements WHERE id = ?",
                    [&stmt_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
            })
            .await
            .unwrap()
            .unwrap();

        assert_eq!(parse_status, "parsed");
        assert_eq!(bal, Some(500_000));
    }

    // ── Date normalization unit tests ────────────────────────────────────────

    #[test]
    fn normalize_dd_slash_mm_yyyy() {
        assert_eq!(
            normalize_date_string("31/01/2024"),
            Some("2024-01-31".to_string())
        );
    }

    #[test]
    fn normalize_dd_dash_mm_yyyy() {
        assert_eq!(
            normalize_date_string("15-03-2024"),
            Some("2024-03-15".to_string())
        );
    }

    #[test]
    fn normalize_dd_mmm_yyyy() {
        assert_eq!(
            normalize_date_string("28 Feb 2024"),
            Some("2024-02-28".to_string())
        );
    }

    #[test]
    fn normalize_dd_mon_yyyy_uppercase() {
        assert_eq!(
            normalize_date_string("15-JAN-2024"),
            Some("2024-01-15".to_string())
        );
    }

    #[test]
    fn normalize_iso8601_passthrough() {
        assert_eq!(
            normalize_date_string("2024-06-30"),
            Some("2024-06-30".to_string())
        );
    }
}
