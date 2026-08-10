//! Recovers a statement's identity: issuer, account, and billing period.
//!
//! This is what attributes a statement to an instrument. When identification
//! fails the statement is not guessed into an arbitrary account -- the user is
//! asked instead, since a misattributed statement would corrupt the balances of
//! two accounts at once.
use crate::extraction::normalization::clean_masked_identifier;
use crate::statements::parser::ParsedPage;
use anyhow::Result;
use regex::Regex;

#[derive(Debug, Default)]
pub struct StatementMetadata {
    pub billing_period_start: Option<String>,
    pub billing_period_end: Option<String>,
    pub due_date: Option<String>,
    pub minimum_due: Option<i64>,
    pub current_balance: Option<i64>,
    pub issuer_name: Option<String>,
    pub masked_identifier: Option<String>,
    pub network: Option<String>,
    pub rewards_summary_json: Option<String>,
    pub statement_date: Option<String>,
}

/// Extracts a statement's identity: issuer, account, period and balances.
pub fn extract_metadata(pages: &[ParsedPage]) -> Result<StatementMetadata> {
    let mut meta = StatementMetadata::default();

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

    meta.issuer_name = detect_issuer(header_text);

    let period_labels = r"(?:billing|statement)\s+period";
    meta.billing_period_start = extract_date_pattern(
        header_text,
        &[
            format!(r"(?i){period_labels}\s*:?\s+({DATE})"),
            r"(?i)billing\s+from[:\s]+(\d{1,2}[/\-]\d{1,2}[/\-]\d{2,4})".to_string(),
            r"(?i)from\s+date[:\s]+(\d{1,2}[/\-]\d{1,2}[/\-]\d{2,4})".to_string(),
            r"(?i)period\s+from[:\s]+(\d{4}-\d{2}-\d{2})".to_string(),
        ],
    )
    .or_else(|| value_below_label(header_text, period_labels, DATE));
    meta.billing_period_end = extract_date_pattern(
        header_text,
        &[
            format!(r"(?i){period_labels}\s*:?\s+(?:{DATE})\s*(?:-|–|—|to)\s*({DATE})"),
            r"(?i)billing\s+to[:\s]+(\d{1,2}[/\-]\d{1,2}[/\-]\d{2,4})".to_string(),
            r"(?i)to\s+date[:\s]+(\d{1,2}[/\-]\d{1,2}[/\-]\d{2,4})".to_string(),
            r"(?i)period\s+to[:\s]+(\d{4}-\d{2}-\d{2})".to_string(),
        ],
    );

    meta.statement_date = extract_date_pattern(
        header_text,
        &[format!(
            r"(?i)statement\s+(?:date|generation\s+date)\s*:?\s+({DATE})"
        )],
    )
    .or_else(|| value_below_label(header_text, r"statement\s+(?:generation\s+)?date", DATE))
    .and_then(|d| normalize_date_string(&d));

    if let Some(ref d) = meta.billing_period_start.clone() {
        meta.billing_period_start = normalize_date_string(d);
    }
    if let Some(ref d) = meta.billing_period_end.clone() {
        meta.billing_period_end = normalize_date_string(d);
    }

    let due_labels = r"(?:payment\s+due\s+date|due\s+date|pay\s+by|amount\s+due\s+date)";
    meta.due_date =
        extract_date_pattern(header_text, &[format!(r"(?i){due_labels}\s*:?\s+({DATE})")])
            .or_else(|| value_below_label(header_text, due_labels, DATE));
    if let Some(ref d) = meta.due_date.clone() {
        meta.due_date = normalize_date_string(d);
    }

    const AMOUNT: &str = r"\d[\d,]*\.\d{2}";
    let currency = r"(?:\(\s*[`\x{20B9}]?\s*\)|INR|Rs\.?|[`\x{20B9}])?";
    let min_labels = r"\*{0,2}min(?:imum)?\s+(?:amount\s+|payment\s+)?due";
    meta.minimum_due = extract_amount_minor(
        header_text,
        &[format!(
            r"(?i){min_labels}\s*{currency}\s*:?\s*{currency}\s*({AMOUNT})"
        )],
    )
    .or_else(|| value_below_label(header_text, min_labels, AMOUNT).and_then(|v| parse_amount(&v)));

    let total_labels = r"\*{0,2}(?:total\s+(?:amount|payment)\s+due|outstanding\s+balance|current\s+balance|closing\s+balance|amount\s+payable)";
    meta.current_balance = extract_amount_minor(
        header_text,
        &[format!(
            r"(?i){total_labels}\s*{currency}\s*:?\s*{currency}\s*({AMOUNT})"
        )],
    )
    .or_else(|| {
        value_below_label(header_text, total_labels, AMOUNT).and_then(|v| parse_amount(&v))
    });

    meta.masked_identifier = extract_text_pattern(
        header_text,
        &[
            r"(?i)card\s+(?:ending|number|no\.?)\s*:?\s*([\dXx*][\dXx*\s\-]{3,24}\d)",
            r"(?i)a/c\s+(?:no\.?|number|ending)\s*:?\s*([\dXx*][\dXx*\s\-]{3,24}\d)",
            r"(?i)account\s+(?:no\.?|number|ending)\s*:?\s*([\dXx*][\dXx*\s\-]{3,24}\d)",
            r"(?i)([Xx*]{2,}[\dXx*\s\-]*\d{2,4})\b",
        ],
    )
    .map(|s| clean_masked_identifier(&s))
    .filter(|s| !s.is_empty());

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

/// Identifies the issuing bank from statement text.
fn detect_issuer(text: &str) -> Option<String> {
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

/// Normalises a date string to ISO form.
fn normalize_date_string(date: &str) -> Option<String> {
    crate::statements::row_extractor::parse_date(date)
}

const DATE: &str =
    r"\d{1,2}[/\-.]\d{1,2}[/\-.]\d{2,4}|\d{1,2}[\s\-/][A-Za-z]{3}[\s\-/]?,?\s?\d{2,4}";

/// Finds the first date matching any of the supplied patterns.
fn extract_date_pattern<S: AsRef<str>>(text: &str, patterns: &[S]) -> Option<String> {
    for pat in patterns {
        if let Ok(re) = Regex::new(pat.as_ref()) {
            if let Some(caps) = re.captures(text) {
                if let Some(m) = caps.get(1) {
                    return Some(m.as_str().to_string());
                }
            }
        }
    }
    None
}

/// Parses an amount into minor units.
fn parse_amount(raw: &str) -> Option<i64> {
    let cleaned = raw.replace(',', "");
    cleaned
        .parse::<f64>()
        .ok()
        .map(|v| (v * 100.0).round() as i64)
}

/// Finds the first amount matching any of the supplied patterns.
fn extract_amount_minor<S: AsRef<str>>(text: &str, patterns: &[S]) -> Option<i64> {
    for pat in patterns {
        if let Ok(re) = Regex::new(pat.as_ref()) {
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

/// Reads a value positioned beneath its label.
///
/// Statement layouts frequently place a label above its value rather than beside
/// it, which a same-line pattern would miss entirely.
fn value_below_label(text: &str, label: &str, value: &str) -> Option<String> {
    let label_re = Regex::new(&format!(r"(?i){label}")).ok()?;
    let value_re = Regex::new(&format!(r"(?i){value}")).ok()?;
    let lines: Vec<&str> = text.lines().collect();

    for (index, line) in lines.iter().enumerate() {
        let Some(found) = label_re.find(line) else {
            continue;
        };
        if value_re.find_at(line, found.end()).is_some() {
            continue;
        }
        let label_column = line[..found.start()].chars().count();

        for below in lines.iter().skip(index + 1).take(2) {
            let best = value_re
                .find_iter(below)
                .map(|m| {
                    let column = below[..m.start()].chars().count();
                    (column.abs_diff(label_column), m.as_str().to_string())
                })
                .filter(|(distance, _)| *distance <= 24)
                .min_by_key(|(distance, _)| *distance);
            if let Some((_, matched)) = best {
                return Some(matched);
            }
        }
    }
    None
}

/// Finds the first text matching any of the supplied patterns.
fn extract_text_pattern<S: AsRef<str>>(text: &str, patterns: &[S]) -> Option<String> {
    for pat in patterns {
        if let Ok(re) = Regex::new(pat.as_ref()) {
            if let Some(caps) = re.captures(text) {
                if let Some(m) = caps.get(1) {
                    return Some(m.as_str().to_string());
                }
            }
        }
    }
    None
}

/// Writes the statement row derived from extracted metadata.
pub async fn write_statement_row(
    stmt_id: &str,
    instrument_id: &str,
    instrument_type: &str,
    meta: &StatementMetadata,
    source_message_id: Option<&str>,
    pool: &deadpool_sqlite::Pool,
) -> Result<String> {
    let sid = stmt_id.to_string();
    let inst_id = instrument_id.to_string();
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

/// Resolves the statement's instrument, creating one if warranted.
///
/// Where identification is not confident the user is asked instead. A
/// misattributed statement corrupts the balances of two accounts at once, so
/// guessing is the worse option.
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
            c.execute(
                "INSERT OR IGNORE INTO instruments \
                 (id, type, issuer_name, masked_identifier, network, status, created_at, updated_at) \
                 VALUES (?, ?, ?, ?, ?, 'active', datetime('now'), datetime('now'))",
                rusqlite::params![id, itype, issuer, masked, net],
            )?;
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

#[cfg(test)]
mod real_statement_headers {
    use super::*;

    fn meta_of(text: &str) -> StatementMetadata {
        extract_metadata(&[ParsedPage {
            page_number: 1,
            text: text.to_string(),
            ocr_used: false,
        }])
        .unwrap()
    }

    #[test]
    fn a_masked_card_yields_its_tail_not_its_bin() {
        for (header, want) in [
            ("Credit Card No.       526873XXXXXX0364", "0364"),
            (
                "Card No:    533467******9740     Name  ADITYA RAWAL",
                "9740",
            ),
            ("Credit Card No. 4147XXXXXXXX7480)", "7480"),
            (
                "Statement for YES BANK Card Number 3561XXXXXXXX2982",
                "2982",
            ),
            ("Credit Card Number\nXXXX XXXX XXXX XX03", "03"),
            ("Card Number: XXXX 3620", "3620"),
        ] {
            assert_eq!(
                meta_of(header).masked_identifier.as_deref(),
                Some(want),
                "for {header:?}"
            );
        }
    }

    #[test]
    fn hdfc_billing_period_and_statement_date() {
        let meta = meta_of(
            "ADITYA RAWAL          Credit Card No.        526873XXXXXX0364\n\
             Statement Date                        13 May, 2026\n\
             ALLAHABAD 211011 UP       Billing Period      14 Apr, 2026 - 13 May, 2026\n",
        );
        assert_eq!(meta.billing_period_start.as_deref(), Some("2026-04-14"));
        assert_eq!(meta.billing_period_end.as_deref(), Some("2026-05-13"));
        assert_eq!(meta.statement_date.as_deref(), Some("2026-05-13"));
    }

    #[test]
    fn sbi_period_and_amounts_across_a_currency_annotation() {
        let meta = meta_of(
            "for Statement Period: 10 Jun 26 to 09 Jul 26\n\
             *Total Amount Due ( ` )\n\
             3,567.00\n\
             **Minimum Amount Due ( ` )\n\
             200.00\n",
        );
        assert_eq!(meta.billing_period_start.as_deref(), Some("2026-06-10"));
        assert_eq!(meta.billing_period_end.as_deref(), Some("2026-07-09"));
        assert_eq!(meta.current_balance, Some(356_700));
        assert_eq!(meta.minimum_due, Some(20_000));
    }

    #[test]
    fn axis_values_are_read_from_the_column_below_their_label() {
        let meta = meta_of(
            "Total Payment Due      Minimum Payment Due      Payment Due Date\n\
             16,188.72              15,612.00                01/08/2026\n",
        );
        assert_eq!(meta.current_balance, Some(1_618_872));
        assert_eq!(meta.minimum_due, Some(1_561_200));
        assert_eq!(meta.due_date.as_deref(), Some("2026-08-01"));
    }

    #[test]
    fn a_reference_number_is_not_read_as_an_amount() {
        let meta = meta_of(
            "STMT No.        : A26070900725\n\
             **Minimum Amount Due ( ` )\n\
             STMT No.        : A26070900725\n",
        );
        assert_eq!(meta.minimum_due, None);
    }

    #[test]
    fn a_same_line_value_is_not_overridden_by_the_line_below() {
        let meta = meta_of(
            "Payment Due Date: 03/08/2026          YES ONLINE\n\
             Statement Date : 14/07/2026\n",
        );
        assert_eq!(meta.due_date.as_deref(), Some("2026-08-03"));
    }
}

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
            Some(850_000),
            "current_balance must be 850000 paise"
        );
        assert_eq!(
            meta.minimum_due,
            Some(50_000),
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

    #[test]
    fn test_partial_metadata_still_persists_statement_row() {
        let text = "HDFC Bank Statement\n\
                    Statement Period: 01/01/2024 to\n\
                    VISA";
        let pages = vec![make_page(text)];
        let meta =
            extract_metadata(&pages).expect("extract_metadata must not fail on partial data");

        assert_eq!(
            meta.billing_period_start.as_deref(),
            Some("2024-01-01"),
            "billing_period_start must be extracted even if end is missing"
        );
        assert!(
            meta.due_date.is_none() || meta.due_date.is_some(),
            "due_date absence must not cause rejection"
        );
    }

    #[tokio::test]
    async fn test_auto_create_instrument_from_statement() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let pool = crate::db::init_db(temp_dir.join("test.db")).await.unwrap();

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

    #[tokio::test]
    async fn test_write_statement_row_partial_metadata() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("test.db");
        let pool = crate::db::init_db(db_path).await.unwrap();

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
