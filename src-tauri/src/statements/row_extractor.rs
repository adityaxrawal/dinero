//! Pulls transaction rows out of a reconstructed statement page.
//!
//! Bank-specific parsers handle the layouts that differ enough to need them,
//! with a generic path for the rest. Rows are then mapped to statement entries,
//! the form the reconciliation pipeline consumes.
use crate::statements::parser::ParsedPage;
use anyhow::Result;
use std::sync::OnceLock;
use regex::Regex;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StatementRow {
    pub transaction_date: String,
    pub merchant_raw: String,
    pub amount_minor: i64,
    pub currency: String,
    pub direction: String,
    pub reference_id: Option<String>,
    pub row_index: usize,
    pub llm_extracted: bool,
}

/// Extracts transaction rows from a parsed statement.
pub fn extract_rows(pages: &[ParsedPage]) -> Result<Vec<StatementRow>> {
    // One deterministic parser for every bank. The bank-specific parsers it
    // replaced matched none of the owner's real statements (audit F-2).
    let merged_pages: Vec<ParsedPage> = pages
        .iter()
        .map(|p| ParsedPage {
            page_number: p.page_number,
            text: merge_broken_line_boundaries(&p.text),
            ocr_used: p.ocr_used,
        })
        .collect();
    let rows = parse_universal(&merged_pages);
    tracing::info!("Extracted {} statement rows", rows.len());
    Ok(rows)
}

/// Whether a row should be excluded as non-transactional.
///
/// Filters carried-forward balances, totals and interest summaries. These sit in
/// the same table as real transactions and would otherwise be ingested as
/// spending that never happened.
pub(crate) fn is_excluded_row(description: &str) -> bool {
    let d: String = description
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let d = d.as_str();
    d.is_empty()
        || d.contains("opening balance")
        || d.contains("closing balance")
        || d.contains("previous balance")
        || d.contains("brought forward")
        || d.contains("carried forward")
        || d.contains("total transactions")
        || d.contains("grand total")
        || d.contains("sub total")
        || d.contains("subtotal")
        || d.contains("total amount")
        || d.contains("reward point")
        || d.contains("loyalty point")
        || d.contains("payment received")
        || d.contains("payment thank")
        || d.contains("cr limit")
        || d.contains("credit limit")
        || d == "date"
        || d == "description"
        || d == "amount"
        || d == "dr"
        || d == "cr"
        || d.trim().is_empty()
}

/// Parses a statement date in whichever format the bank used.
pub(crate) fn parse_date(s: &str) -> Option<String> {
    use chrono::NaiveDate;
    let s = s.trim();
    let formats = [
        "%d/%m/%y",
        "%d-%m-%y",
        "%d.%m.%y",
        "%d %b %y",
        "%d-%b-%y",
        "%d/%b/%y",
        "%d %b, %y",
        "%d/%m/%Y",
        "%d-%m-%Y",
        "%d.%m.%Y",
        "%d %b %Y",
        "%d-%b-%Y",
        "%d/%b/%Y",
        "%Y-%m-%d",
        "%d %b, %Y",
    ];
    for fmt in &formats {
        if let Ok(d) = NaiveDate::parse_from_str(s, fmt) {
            return Some(d.format("%Y-%m-%d").to_string());
        }
    }
    let upper = s.to_uppercase();
    if let Ok(d) = NaiveDate::parse_from_str(&upper, "%d-%b-%Y") {
        return Some(d.format("%Y-%m-%d").to_string());
    }
    None
}

/// Parses an amount into integer minor units.
pub(crate) fn parse_amount_minor(s: &str) -> Option<i64> {
    let cleaned = s
        .replace("INR", "")
        .replace("Rs.", "")
        .replace("Rs", "")
        .replace(['₹', ','], "")
        .trim()
        .to_string();
    cleaned
        .parse::<f64>()
        .ok()
        .map(|v| (v * 100.0).round() as i64)
}

/// Rejoins a transaction split across two lines by the PDF's layout.
///
/// A date alone on one line with its description on the next is a line-wrapping
/// artefact, not two transactions. The following line is only merged when it does
/// not itself begin with a date, which is what prevents two genuine consecutive
/// transactions being collapsed into one.
fn merge_broken_line_boundaries(text: &str) -> String {
    static DATE_ONLY: OnceLock<Regex> = OnceLock::new();
    static STARTS_WITH_DATE: OnceLock<Regex> = OnceLock::new();
    let date_only_re = DATE_ONLY.get_or_init(|| {
        Regex::new(r"^\s*(?:\d{2}[/\-]\d{2}[/\-]\d{4}|\d{2}-[A-Za-z]{3}-\d{4})\s*$")
            .expect("static regex must compile")
    });
    let starts_with_date_re = STARTS_WITH_DATE.get_or_init(|| {
        Regex::new(r"^\s*(?:\d{2}[/\-]\d{2}[/\-]\d{4}|\d{2}-[A-Za-z]{3}-\d{4})")
            .expect("static regex must compile")
    });

    let lines: Vec<&str> = text.lines().collect();
    let mut merged_lines: Vec<String> = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if date_only_re.is_match(line) && i + 1 < lines.len() {
            let next = lines[i + 1];
            if !next.trim().is_empty() && !starts_with_date_re.is_match(next) {
                merged_lines.push(format!("{} {}", line.trim(), next.trim()));
                i += 2;
                continue;
            }
        }
        merged_lines.push(line.to_string());
        i += 1;
    }
    merged_lines.join("\n")
}

/// The deterministic row parser for every bank (audit F-2).
///
/// Deliberately conservative: it is better to extract fewer rows the user can add
/// to than to invent transactions from a layout it does not really understand.
fn parse_universal(pages: &[ParsedPage]) -> Vec<StatementRow> {
    let re_row = match Regex::new(
        r"(?x)
        ^(?P<date>
            \d{1,2}[/\-.]\d{1,2}[/\-.]\d{2,4}          # 27/06/2026, 4-3-26
          | \d{1,2}[\s\-/][A-Za-z]{3}[\s\-/]?,?\s?\d{2,4} # 12 Jun 26, 13-May-2026, 21/Oct/2025
         )
        (?:\s*\|\s*\d{1,2}:\d{2}(?::\d{2})?)?          # HDFC's '| 00:00' time
        \s+
        (?P<desc>\S.*?)??                               # description (+ any category column);
                                                        # optional — HDFC and IDFC wrap it onto
                                                        # the neighbouring lines, leaving none
                                                        # here (see `wrapped_description`)
        \s*
        (?P<sign>[+\-])?\s*                             # HDFC marks credits with '+'
        (?:[C`\x{20B9}]|Rs\.?|INR)?\s*                  # currency glyph; HDFC's decodes as 'C'
        (?P<amount>\d[\d,]*\.\d{2})
        (?:\s+(?P<dir>Dr|Cr|DR|CR|D|C))?                # Axis/IDFC 'Dr', SBI 'D'
        (?:\s+\S{1,2})?                                 # HDFC's trailing bullet glyph
        \s*$
        ",
    ) {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("parse_universal regex failed to compile: {e}");
            return vec![];
        }
    };

    let mut rows = Vec::new();

    for page in pages {
        let lines: Vec<&str> = page.text.lines().map(|l| l.trim()).collect();
        let is_row: Vec<bool> = lines.iter().map(|l| re_row.is_match(l)).collect();

        for (index, line) in lines.iter().enumerate() {
            let Some(caps) = re_row.captures(line) else {
                continue;
            };
            let inline = caps.name("desc").map(|m| m.as_str().trim()).unwrap_or("");
            let desc = if inline.is_empty() {
                wrapped_description(&lines, &is_row, index)
            } else {
                inline.to_string()
            };
            if is_excluded_row(&desc) || !has_enough_letters(&desc) {
                continue;
            }
            let (Some(transaction_date), Some(amount_minor)) = (
                caps.name("date").and_then(|m| parse_date(m.as_str())),
                caps.name("amount")
                    .and_then(|m| parse_amount_minor(m.as_str())),
            ) else {
                continue;
            };

            rows.push(StatementRow {
                transaction_date,
                merchant_raw: merchant_column(&desc),
                amount_minor,
                currency: "INR".to_string(),
                direction: resolve_direction(
                    caps.name("dir").map(|m| m.as_str()),
                    caps.name("sign").map(|m| m.as_str()),
                )
                .to_string(),
                reference_id: extract_reference_id(&desc),
                row_index: rows.len(),
                llm_extracted: false,
            });
        }
    }
    rows
}

/// Recovers a description wrapped onto the neighbouring line.
///
/// Looks at the lines either side, skipping ones that are themselves transactions
/// or that look like totals. The letter-count requirement rejects stray numeric
/// fragments that are not description text.
fn wrapped_description(lines: &[&str], is_row: &[bool], index: usize) -> String {
    let usable = |i: usize| -> Option<&str> {
        let candidate = lines.get(i)?;
        if *is_row.get(i)? || candidate.contains(char::is_numeric) && looks_like_a_total(candidate)
        {
            return None;
        }
        (candidate.chars().filter(|c| c.is_alphabetic()).count() >= 2).then_some(*candidate)
    };

    [index.checked_sub(1).and_then(usable), usable(index + 1)]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether a line looks like a total rather than a transaction.
fn looks_like_a_total(line: &str) -> bool {
    // Runs per line: compiled once (audit D-2).
    static AMOUNT: OnceLock<Regex> = OnceLock::new();
    AMOUNT
        .get_or_init(|| Regex::new(r"\d[\d,]*\.\d{2}").expect("static regex must compile"))
        .is_match(line)
}

/// Resolves debit or credit from an explicit marker or a sign.
///
/// The CR/DR marker is authoritative where present. Defaulting to debit is the
/// safe direction: over-counting spending is more visible to the user, and
/// therefore more correctable, than silently recording a charge as income.
fn resolve_direction(marker: Option<&str>, sign: Option<&str>) -> &'static str {
    match marker.map(|m| m.to_ascii_uppercase()).as_deref() {
        Some("CR") | Some("C") => return "credit",
        Some("DR") | Some("D") => return "debit",
        _ => {}
    }
    match sign {
        Some("+") => "credit",
        _ => "debit",
    }
}

/// Extracts the merchant portion from a description column.
///
/// Splits on the wide gap that separates columns in a reconstructed layout, and
/// falls back to the whole description when the first segment holds too little
/// text to be a name.
fn merchant_column(description: &str) -> String {
    let first = description
        .split("   ")
        .find(|segment| !segment.trim().is_empty())
        .unwrap_or(description)
        .trim();
    if has_enough_letters(first) {
        first.to_string()
    } else {
        description.trim().to_string()
    }
}

/// Whether a string has enough letters to be a description rather than a code.
fn has_enough_letters(description: &str) -> bool {
    description.chars().filter(|c| c.is_alphabetic()).count() >= 3
}

/// Extracts a twelve-digit reference id from a description.
pub(crate) fn extract_reference_id(description: &str) -> Option<String> {
    // Runs per row: compiled once (audit D-2).
    static REFERENCE: OnceLock<Regex> = OnceLock::new();
    REFERENCE
        .get_or_init(|| Regex::new(r"\b(\d{12})\b").expect("static regex must compile"))
        .captures(description)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

/// Inserts statement entries on a caller-supplied connection, failing on the
/// first row that cannot be saved (used inside the statement commit, where a
/// lost row must fail the whole commit — review H7). Entry ids are derived
/// from (statement, row) so re-running is idempotent.
pub fn insert_statement_entries_sync(
    c: &rusqlite::Connection,
    statement_id: &str,
    rows: &[StatementRow],
) -> rusqlite::Result<Vec<String>> {
    let mut ids = Vec::with_capacity(rows.len());
    for row in rows {
        let entry_id = statement_entry_id(statement_id, row.row_index);
        c.execute(
            "INSERT INTO statement_entries \
             (id, statement_id, row_index, transaction_date, description_raw, \
              merchant_raw, amount, amount_minor, currency, direction, reference_id, \
              raw_row_json, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, '{}', datetime('now')) \
             ON CONFLICT(id) DO NOTHING",
            rusqlite::params![
                entry_id,
                statement_id,
                row.row_index as i64,
                row.transaction_date,
                row.merchant_raw,
                row.merchant_raw,
                row.amount_minor as f64 / 100.0,
                row.amount_minor,
                row.currency,
                row.direction,
                row.reference_id,
            ],
        )?;
        ids.push(entry_id);
    }
    Ok(ids)
}

/// Deterministic statement entry id for (statement, row index).
pub fn statement_entry_id(statement_id: &str, row_index: usize) -> String {
    uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_OID,
        format!("statement_entry:{}:{}", statement_id, row_index).as_bytes(),
    )
    .to_string()
}

/// Maps extracted rows onto statement entries for reconciliation.
pub async fn map_rows_to_statement_entries(
    statement_id: &str,
    rows: &[StatementRow],
    pool: &deadpool_sqlite::Pool,
) -> Vec<String> {
    let conn = match pool.get().await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to get DB connection for statement_entries: {}", e);
            return vec![];
        }
    };

    let stmt_id = statement_id.to_string();
    let rows_owned = rows.to_vec();

    let ids = conn
        .interact(move |c| {
            let mut ids: Vec<String> = Vec::new();
            for row in &rows_owned {
                // Derived from (statement, row) so re-committing the same
                // statement rewrites the same entry instead of adding another,
                // and observations keyed on the entry id dedupe (review H7).
                let entry_id = statement_entry_id(&stmt_id, row.row_index);
                let ref_id = row.reference_id.clone();
                let result = c.execute(
                    "INSERT INTO statement_entries \
                     (id, statement_id, row_index, transaction_date, description_raw, \
                      merchant_raw, amount, amount_minor, currency, direction, reference_id, \
                      raw_row_json, created_at) \
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, '{}', datetime('now')) \
                     ON CONFLICT(id) DO NOTHING",
                    rusqlite::params![
                        entry_id,
                        stmt_id,
                        row.row_index as i64,
                        row.transaction_date,
                        row.merchant_raw,
                        row.merchant_raw,
                        row.amount_minor as f64 / 100.0,
                        row.amount_minor,
                        row.currency,
                        row.direction,
                        ref_id,
                    ],
                );
                match result {
                    Ok(_) => ids.push(entry_id),
                    Err(e) => {
                        tracing::warn!(
                            "Failed to insert statement_entry for row_index={}: {}",
                            row.row_index,
                            e
                        );
                    }
                }
            }
            Ok::<_, rusqlite::Error>(ids)
        })
        .await;

    match ids {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
            tracing::error!("statement_entries batch error: {}", e);
            vec![]
        }
        Err(e) => {
            tracing::error!("DB interact error for statement_entries: {}", e);
            vec![]
        }
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
    fn real_axis_row_shape_is_parsed() {
        let pages = vec![make_page(
            "04/07/2026    AIRTEL PAYMENTS BANK L,GURGAON      UTILITIES     588.82 Dr\n\
             05/07/2026    AMAZON REFUND MUMBAI                RETAIL      1,200.00 Cr\n\
             01/07/2026    BBPS PAYMENT RECEIVED - PU016182HK9XJLQE9713    15,636.26 Cr\n",
        )];
        let rows = parse_universal(&pages);
        assert_eq!(rows.len(), 2, "got {rows:?}");
        assert_eq!(rows[0].transaction_date, "2026-07-04");
        assert_eq!(rows[0].amount_minor, 58_882);
        assert_eq!(rows[0].direction, "debit");
        assert_eq!(rows[0].merchant_raw, "AIRTEL PAYMENTS BANK L,GURGAON");
        assert_eq!(rows[1].direction, "credit");
        assert_eq!(rows[1].amount_minor, 120_000);
    }

    #[test]
    fn real_sbi_row_shape_is_parsed() {
        let pages = vec![make_page(
            "12 Jun 26   ASSPL         Bangalore   IN            304.00  D\n\
             09 Jun 26   CARD CASHBACK CREDIT                    250.00  C\n",
        )];
        let rows = parse_universal(&pages);
        assert_eq!(rows.len(), 2, "got {rows:?}");
        assert_eq!(rows[0].transaction_date, "2026-06-12");
        assert_eq!(rows[0].direction, "debit");
        assert_eq!(rows[1].direction, "credit");
    }

    #[test]
    fn real_hdfc_row_shape_is_parsed() {
        let pages = vec![make_page(
            "14/04/2026| 00:00    10% Swiggy Cashback_Reversal        C 21.50   l\n\
             14/04/2026| 00:00    10% Swiggy Cashback              +  C 30.30   l\n",
        )];
        let rows = parse_universal(&pages);
        assert_eq!(rows.len(), 2, "got {rows:?}");
        assert_eq!(rows[0].transaction_date, "2026-04-14");
        assert_eq!(rows[0].amount_minor, 2_150);
        assert_eq!(rows[0].direction, "debit");
        assert_eq!(rows[1].amount_minor, 3_030);
        assert_eq!(rows[1].direction, "credit", "a leading '+' means credit");
    }

    #[test]
    fn a_description_wrapped_around_the_row_is_recovered() {
        let pages = vec![make_page(
            "Purchases, EMIs & Other Debits\n\
             PAI INTERNATIONAL - Interest Amount\n\
             25 Oct 25                        82.48 DR\n\
             Amortization - <3/3>\n",
        )];
        let rows = parse_universal(&pages);
        assert_eq!(rows.len(), 1, "got {rows:?}");
        assert_eq!(
            rows[0].merchant_raw,
            "PAI INTERNATIONAL - Interest Amount Amortization - <3/3>"
        );
        assert_eq!(rows[0].amount_minor, 8_248);
    }

    #[test]
    fn a_billing_period_line_is_not_a_transaction() {
        let pages = vec![make_page(
            "15/06/2026 To 14/07/2026                        Rs. 1,08,000.00\n",
        )];
        assert!(parse_universal(&pages).is_empty());
    }

    #[test]
    fn real_date_formats_all_parse() {
        for (input, want) in [
            ("27/06/2026", "2026-06-27"),
            ("12 Jun 26", "2026-06-12"),
            ("25 Oct 25", "2025-10-25"),
            ("13 May, 2026", "2026-05-13"),
            ("21/Oct/2025", "2025-10-21"),
            ("13-05-2026", "2026-05-13"),
        ] {
            assert_eq!(parse_date(input).as_deref(), Some(want), "for {input}");
        }
    }

    #[test]
    fn test_statement_row_json_round_trip() {
        let row = StatementRow {
            transaction_date: "2026-06-10".to_string(),
            merchant_raw: "AMAZON PAY".to_string(),
            amount_minor: 150000,
            currency: "INR".to_string(),
            direction: "debit".to_string(),
            reference_id: Some("REF123".to_string()),
            row_index: 0,
            llm_extracted: false,
        };
        let json = serde_json::to_string(&row).unwrap();
        let back: StatementRow = serde_json::from_str(&json).unwrap();
        assert_eq!(row, back);
    }

    #[test]
    fn test_broken_ocr_row_boundary_merged() {
        let text = "HDFC Bank Credit Card Statement\n\
                    01/12/2023\n\
                    SWIGGY ORDER BANGALORE                    1,250.00  DR\n\
                    15/12/2023  BIGBASKET BANGALORE                       2,000.00  DR\n";
        let pages = vec![make_page(text)];
        let rows = extract_rows(&pages).unwrap();

        assert_eq!(
            rows.len(),
            2,
            "the broken-boundary row must be recovered, not silently dropped"
        );
        assert_eq!(rows[0].transaction_date, "2023-12-01");
        assert_eq!(rows[0].merchant_raw, "SWIGGY ORDER BANGALORE");
        assert_eq!(rows[0].amount_minor, 125_000);
        assert_eq!(rows[0].direction, "debit");
    }

    #[test]
    fn test_balance_rows_excluded() {
        let excluded_descs = [
            "Opening Balance",
            "Closing Balance",
            "Previous Balance",
            "Brought Forward",
            "Carried Forward",
            "Grand Total",
            "Sub Total",
            "Total Amount",
            "Reward Points Redeemed",
            "PAYMENT - THANK YOU",
            "PAYMENT RECEIVED - THANK YOU",
            "Sub-total",
            "Opening  Balance",
        ];
        for desc in &excluded_descs {
            assert!(
                is_excluded_row(desc),
                "Row with description '{}' must be excluded",
                desc
            );
        }
        let valid_descs = [
            "AMAZON INDIA NEW DELHI",
            "SWIGGY ORDER",
            "UPI/PHONEPE/TXN123456",
        ];
        for desc in &valid_descs {
            assert!(
                !is_excluded_row(desc),
                "Row with description '{}' must NOT be excluded",
                desc
            );
        }
    }

    #[test]
    fn test_reference_id_extracted_from_description() {
        let desc = "UPI/GOOGLE PAY/123456789012/RECEIVER";
        let ref_id = extract_reference_id(desc);
        assert_eq!(ref_id, Some("123456789012".to_string()));
    }

    #[test]
    fn test_reference_id_absent_when_no_12_digits() {
        let desc = "AMAZON INDIA NEW DELHI";
        let ref_id = extract_reference_id(desc);
        assert_eq!(ref_id, None);
    }

}
