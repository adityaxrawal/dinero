use crate::statements::parser::ParsedPage;
use anyhow::Result;
use regex::Regex;

/// A single extracted statement row (Doc 10 §11.4).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StatementRow {
    pub transaction_date: String, // YYYY-MM-DD UTC
    pub merchant_raw: String,     // Exact text from description column — unmodified
    pub amount_minor: i64,        // Integer minor units (paise)
    pub currency: String,         // ISO 4217 (INR unless FX row)
    pub direction: String,        // "debit" | "credit"
    pub reference_id: Option<String>,
    pub row_index: usize,
    /// Set to true if this row was parsed by LLM-assist fallback (tagged extraction_method=llm_assist)
    pub llm_extracted: bool,
}

/// F10 fix: 7 bank-specific row parser *variants* supported at v1.0 (Doc 10
/// §11.2), covering only **5 distinct banks** (HDFC, ICICI, Axis, Amex, SBI —
/// two variants each for HDFC/ICICI split credit-card vs. bank-account
/// formats). Kotak, a named target bank, has no parser yet — do not read "7"
/// here as "7 banks". Marketing references to "21 banks" are a roadmap
/// target, further still from a v1.0 claim.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BankParser {
    HdfcCreditCard,   // DR/CR column, dd/MM/YYYY, merchant+city in description
    IciciCreditCard,  // Debit/Credit in separate columns
    AxisCreditCard,   // +/- prefix on amount
    AmexIndia,        // US-style column labels
    HdfcBankAccount,  // Balance column; direction inferred from balance delta
    IciciBankAccount, // Similar to HDFC bank account
    SbiCreditCard,    // DD-MON-YYYY date format
    Unknown,          // Falls back to generic OCR/LLM-assist
}

impl BankParser {
    /// Detect which bank parser to use from the issuer name and instrument
    /// type. Doc 30 TASK-QA-003 finding: `issuer_name` here is always
    /// `metadata_extractor::detect_issuer`'s bare canonical code ("HDFC",
    /// "ICICI", ...) or an equally bare `ConfirmedInstrument::issuer_name` --
    /// neither ever contains the literal substring "credit", so the
    /// previous `s.contains("hdfc") && s.contains("credit")`-style checks
    /// could never actually select `HdfcCreditCard`/`IciciCreditCard`
    /// through normal auto-detection, silently routing every real HDFC/
    /// ICICI *credit card* statement to the wrong (bank-account) parser.
    /// `instrument_type` (already resolved separately by the Statement
    /// Instrument Gate before this is ever called) is the correct signal
    /// to disambiguate the two variants per bank.
    pub fn detect(issuer_name: &str, instrument_type: &str) -> Self {
        let lower = issuer_name.to_lowercase();
        let is_credit_card = instrument_type == "credit_card";
        match lower.as_str() {
            s if s.contains("hdfc") && is_credit_card => BankParser::HdfcCreditCard,
            s if s.contains("icici") && is_credit_card => BankParser::IciciCreditCard,
            s if s.contains("axis") => BankParser::AxisCreditCard,
            s if s.contains("amex") => BankParser::AmexIndia,
            s if s.contains("hdfc") => BankParser::HdfcBankAccount,
            s if s.contains("icici") => BankParser::IciciBankAccount,
            s if s.contains("sbi") => BankParser::SbiCreditCard,
            _ => BankParser::Unknown,
        }
    }
}

/// Extracts transaction rows from parsed pages using the detected bank parser.
///
/// Row classification filters (Doc 10 §11.5) — excluded from extraction:
///   - Opening and closing balance rows
///   - Total or subtotal rows
///   - Reward redemption or adjustment rows (not real-world transactions)
///   - Blank separator rows
///
/// Broken row boundary handling (Doc 10 §11.6):
///   - Rows with only a date and no amount are merged with the next row's amount.
///
/// Individual row failures are isolated — they do NOT stop the rest of the statement
/// from being processed (Doc 10 §16 invariant).
pub fn extract_rows(pages: &[ParsedPage], parser: BankParser) -> Result<Vec<StatementRow>> {
    tracing::info!("Extracting rows using parser: {:?}", parser);

    // Doc 30 TASK-STMT-005: broken-boundary merging applied once, generically,
    // before any bank-specific regex runs — every one of the 7 parsers
    // benefits without needing its own look-ahead logic (see
    // `merge_broken_line_boundaries`'s own doc comment for why this replaced
    // the earlier `merge_broken_rows` helper, which operated on an
    // intermediate tuple representation none of the real parsers produced).
    let merged_pages: Vec<ParsedPage> = pages
        .iter()
        .map(|p| ParsedPage {
            page_number: p.page_number,
            text: merge_broken_line_boundaries(&p.text),
            ocr_used: p.ocr_used,
        })
        .collect();
    let pages = &merged_pages[..];

    let mut rows: Vec<StatementRow> = match parser {
        BankParser::HdfcCreditCard => parse_hdfc_credit(pages),
        BankParser::IciciCreditCard => parse_icici_credit(pages),
        BankParser::AxisCreditCard => parse_axis_credit(pages),
        BankParser::AmexIndia => parse_amex_india(pages),
        BankParser::HdfcBankAccount => parse_hdfc_account(pages),
        BankParser::IciciBankAccount => parse_icici_account(pages),
        BankParser::SbiCreditCard => parse_sbi_credit(pages),
        BankParser::Unknown => Vec::new(),
    };

    // Issue #8: when the bank-specific parser recognises nothing, fall back to
    // the universal row shape rather than giving up.
    //
    // Measured against real statements, none of the seven parsers above match
    // the format their bank actually issues: real Axis rows end in `Dr`/`Cr`
    // where `parse_axis_credit` expects a `+`/`-` prefixed amount, real HDFC
    // rows carry a `| HH:MM` time after the date and mark credits with a
    // leading `+` rather than a trailing `CR`, and real SBI rows use a
    // two-digit year and a single `D`/`C`. They were written against invented
    // samples. Rather than rewrite seven guesses into seven new guesses, the
    // shape they all really share is handled once, in `parse_universal`.
    if rows.is_empty() {
        rows = parse_universal(pages);
    }

    tracing::info!("Extracted {} statement rows", rows.len());
    Ok(rows)
}

// ── Shared utilities ──────────────────────────────────────────────────────────

/// Row type filter: returns `true` if the row should be EXCLUDED (Doc 10 §11.5).
///
/// Excluded row categories:
///   - Opening balance / Closing balance rows
///   - Total / Sub-total / Grand total rows
///   - Blank or separator lines
///   - Reward point redemptions
///   - Fee adjustment notes (not actual transaction rows)
pub(crate) fn is_excluded_row(description: &str) -> bool {
    // Issue #8: punctuation is flattened before matching. Every phrase below
    // is written as plain words, but banks print them with separators —
    // HDFC's card-payment line is "PAYMENT - THANK YOU", which contains
    // neither "payment thank" nor "payment received" as a literal substring
    // and so slipped through as an ordinary debit, double-counting against
    // the payment already captured from the bank account's own statement.
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

/// Parses a date string to YYYY-MM-DD. Handles dd/MM/YYYY, dd-MM-YYYY, dd MMM YYYY,
/// DD-MON-YYYY and YYYY-MM-DD.
pub(crate) fn parse_date(s: &str) -> Option<String> {
    use chrono::NaiveDate;
    let s = s.trim();
    // Issue #8: the two-digit-year and comma forms come from real statements —
    // SBI prints "12 Jun 26" and HDFC prints "13 May, 2026". Without them
    // every SBI row parsed its date as `None` and was silently dropped.
    //
    // The two-digit forms must be tried FIRST. `%Y` will happily consume a
    // bare "26" and yield the year 0026, so "12 Jun 26" parsed against
    // "%d %b %Y" succeeds with a nonsense date instead of falling through.
    // The reverse cannot misfire: `%y` reads exactly two digits, so
    // "12 Jun 2026" leaves "26" unconsumed and is rejected, dropping through
    // to the four-digit forms below.
    let formats = [
        "%d/%m/%y", "%d-%m-%y", "%d.%m.%y", "%d %b %y", "%d-%b-%y", "%d/%b/%y", "%d %b, %y",
        "%d/%m/%Y", "%d-%m-%Y", "%d.%m.%Y", "%d %b %Y", "%d-%b-%Y", "%d/%b/%Y", "%Y-%m-%d",
        "%d %b, %Y",
    ];
    for fmt in &formats {
        if let Ok(d) = NaiveDate::parse_from_str(s, fmt) {
            return Some(d.format("%Y-%m-%d").to_string());
        }
    }
    // Try uppercase month abbreviation (DD-MON-YYYY)
    let upper = s.to_uppercase();
    if let Ok(d) = NaiveDate::parse_from_str(&upper, "%d-%b-%Y") {
        return Some(d.format("%Y-%m-%d").to_string());
    }
    None
}

/// Parses an amount string (Indian locale: commas, optional decimal) to minor units (paise).
/// Strips INR/Rs/₹ prefix and leading/trailing whitespace.
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

/// Doc 30 TASK-STMT-005: broken-boundary merging — "detect a row where
/// date/description captured but amount is missing... and merge it with the
/// immediately following row carrying the orphaned amount," a common
/// PDF-extraction rendering artifact. Operates directly on raw page text,
/// generically across every bank's date format, and is wired into
/// `extract_rows()` itself (unlike the tuple-based `merge_broken_rows` this
/// replaced, which never had a real producer feeding it — none of the 7 bank
/// parsers below build that intermediate representation, so it only ever
/// ran inside its own unit test).
///
/// A "date-only" line (nothing else on it) immediately followed by a line
/// that does *not* itself start with a date is joined into one line before
/// any bank-specific regex sees it.
fn merge_broken_line_boundaries(text: &str) -> String {
    let date_only_re = Regex::new(r"^\s*(?:\d{2}[/\-]\d{2}[/\-]\d{4}|\d{2}-[A-Za-z]{3}-\d{4})\s*$")
        .expect("static regex must compile");
    let starts_with_date_re =
        Regex::new(r"^\s*(?:\d{2}[/\-]\d{2}[/\-]\d{4}|\d{2}-[A-Za-z]{3}-\d{4})")
            .expect("static regex must compile");

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

// ── HDFC Credit Card Parser ───────────────────────────────────────────────────
//
// HDFC CC Statement format:
//   Columns: Date | Description | Amount (INR) | Dr/Cr
//   Date format: dd/MM/YYYY
//   Direction: separate "DR" or "CR" column or inline "(DR)" / "(CR)" suffix
//   Example row:
//     01/12/2023  SWIGGY ORDER BANGALORE              1,250.00  DR
//     15/12/2023  PAYMENT - THANK YOU                20,000.00  CR

fn parse_hdfc_credit(pages: &[ParsedPage]) -> Vec<StatementRow> {
    // Full-line regex: date | description | amount | DR/CR
    let re_row = match Regex::new(
        r"(?x)
        ^(\d{2}/\d{2}/\d{4})\s+       # date
        (.+?)\s+                        # description (non-greedy)
        ([\d,]+\.\d{2})\s+             # amount
        (DR|CR|Dr|Cr)\s*$              # direction
        ",
    ) {
        Ok(r) => r,
        Err(_) => return vec![],
    };

    let mut rows = Vec::new();
    let mut row_index = 0usize;

    for page in pages {
        for line in page.text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Some(caps) = re_row.captures(line) {
                let date_raw = &caps[1];
                let desc = caps[2].trim().to_string();
                let amount_str = &caps[3];
                let dr_cr = caps[4].to_uppercase();

                if is_excluded_row(&desc) {
                    continue;
                }

                let date = match parse_date(date_raw) {
                    Some(d) => d,
                    None => continue,
                };
                let amount_minor = match parse_amount_minor(amount_str) {
                    Some(a) => a,
                    None => continue,
                };
                let direction = if dr_cr == "DR" { "debit" } else { "credit" }.to_string();

                // Extract reference ID before moving desc into the struct
                let reference_id = extract_reference_id(&desc);
                let merchant_raw = desc;

                rows.push(StatementRow {
                    transaction_date: date,
                    merchant_raw,
                    amount_minor,
                    currency: "INR".to_string(),
                    direction,
                    reference_id,
                    row_index,
                    llm_extracted: false,
                });
                row_index += 1;
            }
        }
    }
    rows
}

// ── ICICI Credit Card Parser ──────────────────────────────────────────────────
//
// ICICI CC Statement format:
//   Columns: Transaction Date | Transaction Details | Debit | Credit | Amount
//   Debit column has value, Credit is blank for debits and vice versa.
//   Date format: dd/MM/YYYY or dd-MM-YYYY
//   Example rows:
//     01/12/2023  AMAZON INDIA                     2,500.00              2,500.00
//     10/12/2023  SALARY CREDIT                               50,000.00  50,000.00

fn parse_icici_credit(pages: &[ParsedPage]) -> Vec<StatementRow> {
    // Row: date | description | debit_amount_or_blank | credit_amount_or_blank | total_amount
    // NOTE: use r#"..."# delimiter to allow literal double-quotes inside the regex comment
    let re_row = match Regex::new(
        r#"(?x)
        ^(\d{2}[/\-]\d{2}[/\-]\d{4})\s+   # date
        (.+?)\s{2,}                           # description (2+ spaces separate amounts)
        ([\d,]+\.\d{2}|-)\s+                 # debit amount or dash
        ([\d,]+\.\d{2}|-)\s+                 # credit amount or dash
        ([\d,]+\.\d{2}|-)\s*$                # total amount or dash
        "#,
    ) {
        Ok(r) => r,
        Err(_) => return vec![],
    };

    let mut rows = Vec::new();
    let mut row_index = 0usize;

    for page in pages {
        for line in page.text.lines() {
            let line = line.trim();
            if let Some(caps) = re_row.captures(line) {
                let date_raw = &caps[1];
                let desc = caps[2].trim().to_string();
                let debit_str = caps[3].trim();
                let credit_str = caps[4].trim();

                if is_excluded_row(&desc) {
                    continue;
                }

                let date = match parse_date(date_raw) {
                    Some(d) => d,
                    None => continue,
                };

                // Direction from which column has the value
                let (direction, amount_str) = if debit_str != "-" && !debit_str.is_empty() {
                    ("debit", debit_str)
                } else if credit_str != "-" && !credit_str.is_empty() {
                    ("credit", credit_str)
                } else {
                    continue; // both blank — skip
                };

                let amount_minor = match parse_amount_minor(amount_str) {
                    Some(a) => a,
                    None => continue,
                };

                let reference_id = extract_reference_id(&desc);

                rows.push(StatementRow {
                    transaction_date: date,
                    merchant_raw: desc,
                    amount_minor,
                    currency: "INR".to_string(),
                    direction: direction.to_string(),
                    reference_id,
                    row_index,
                    llm_extracted: false,
                });
                row_index += 1;
            }
        }
    }
    rows
}

// ── Axis Bank Credit Card Parser ──────────────────────────────────────────────
//
// Axis CC format: +/- prefix on amount, dd/MM/YYYY dates.
// Example: 05/12/2023  NETFLIX INDIA MUMBAI  -649.00

fn parse_axis_credit(pages: &[ParsedPage]) -> Vec<StatementRow> {
    let re_row = match Regex::new(
        r"(?x)
        ^(\d{2}/\d{2}/\d{4})\s+   # date
        (.+?)\s+                    # description
        ([+\-][\d,]+\.\d{2})\s*$  # +/- prefixed amount
        ",
    ) {
        Ok(r) => r,
        Err(_) => return vec![],
    };

    let mut rows = Vec::new();
    let mut row_index = 0usize;

    for page in pages {
        for line in page.text.lines() {
            let line = line.trim();
            if let Some(caps) = re_row.captures(line) {
                let date_raw = &caps[1];
                let desc = caps[2].trim().to_string();
                let amount_raw = caps[3].trim();

                if is_excluded_row(&desc) {
                    continue;
                }

                let date = match parse_date(date_raw) {
                    Some(d) => d,
                    None => continue,
                };

                let (direction, amount_str) = if let Some(stripped) = amount_raw.strip_prefix('+') {
                    ("credit", stripped)
                } else {
                    ("debit", &amount_raw[1..])
                };

                let amount_minor = match parse_amount_minor(amount_str) {
                    Some(a) => a,
                    None => continue,
                };

                let reference_id = extract_reference_id(&desc);

                rows.push(StatementRow {
                    transaction_date: date,
                    merchant_raw: desc,
                    amount_minor,
                    currency: "INR".to_string(),
                    direction: direction.to_string(),
                    reference_id,
                    row_index,
                    llm_extracted: false,
                });
                row_index += 1;
            }
        }
    }
    rows
}

// ── AMEX India Parser ─────────────────────────────────────────────────────────
//
// AMEX India format:
//   Date: MM/DD/YYYY (US-style)
//   Amount: positive = debit, negative = credit
//   Example: 12/05/2023  SWIGGY ORDER             1,300.00

fn parse_amex_india(pages: &[ParsedPage]) -> Vec<StatementRow> {
    let re_row = match Regex::new(
        r"(?x)
        ^(\d{2}/\d{2}/\d{4})\s+   # date MM/DD/YYYY
        (.+?)\s+                    # description
        (-?[\d,]+\.\d{2})\s*$     # amount (negative = credit)
        ",
    ) {
        Ok(r) => r,
        Err(_) => return vec![],
    };

    let mut rows = Vec::new();
    let mut row_index = 0usize;

    for page in pages {
        for line in page.text.lines() {
            let line = line.trim();
            if let Some(caps) = re_row.captures(line) {
                let date_raw = &caps[1];
                let desc = caps[2].trim().to_string();
                let amount_raw = caps[3].trim();

                if is_excluded_row(&desc) {
                    continue;
                }

                // AMEX India uses MM/DD/YYYY
                let date = {
                    use chrono::NaiveDate;
                    NaiveDate::parse_from_str(date_raw, "%m/%d/%Y")
                        .ok()
                        .map(|d| d.format("%Y-%m-%d").to_string())
                };
                let date = match date {
                    Some(d) => d,
                    None => continue,
                };

                let is_negative = amount_raw.starts_with('-');
                let clean = amount_raw.trim_start_matches('-');
                let amount_minor = match parse_amount_minor(clean) {
                    Some(a) => a,
                    None => continue,
                };
                // Positive = debit (charge), negative = credit (payment/refund)
                let direction = if is_negative { "credit" } else { "debit" };

                let reference_id = extract_reference_id(&desc);

                rows.push(StatementRow {
                    transaction_date: date,
                    merchant_raw: desc,
                    amount_minor,
                    currency: "INR".to_string(),
                    direction: direction.to_string(),
                    reference_id,
                    row_index,
                    llm_extracted: false,
                });
                row_index += 1;
            }
        }
    }
    rows
}

// ── HDFC Bank Account Parser ──────────────────────────────────────────────────
//
// HDFC Savings/Current account format:
//   Columns: Date | Narration | Chq/Ref No | Value Date | Withdrawal Amt | Deposit Amt | Balance
//   Direction is inferred from which amount column has value.
//   Date: dd/MM/YYYY

fn parse_hdfc_account(pages: &[ParsedPage]) -> Vec<StatementRow> {
    // Match: date | narration | ref_no | value_date | withdrawal | deposit | balance
    let re_row = match Regex::new(
        r"(?x)
        ^(\d{2}/\d{2}/\d{4})\s+   # txn date
        (.+?)\s+                    # narration/description
        (\S*)\s+                    # chq/ref no (may be empty)
        \d{2}/\d{2}/\d{4}\s+       # value date (skip)
        ([\d,]+\.\d{2}|-)\s+       # withdrawal
        ([\d,]+\.\d{2}|-)\s+       # deposit
        [\d,]+\.\d{2}\s*$          # balance (skip)
        ",
    ) {
        Ok(r) => r,
        Err(_) => return vec![],
    };

    let mut rows = Vec::new();
    let mut row_index = 0usize;

    for page in pages {
        for line in page.text.lines() {
            let line = line.trim();
            if let Some(caps) = re_row.captures(line) {
                let date_raw = &caps[1];
                let narration = caps[2].trim().to_string();
                let ref_no = caps[3].trim();
                let withdrawal = caps[4].trim();
                let deposit = caps[5].trim();

                if is_excluded_row(&narration) {
                    continue;
                }

                let date = match parse_date(date_raw) {
                    Some(d) => d,
                    None => continue,
                };

                let (direction, amount_str) = if withdrawal != "-" && !withdrawal.is_empty() {
                    ("debit", withdrawal)
                } else if deposit != "-" && !deposit.is_empty() {
                    ("credit", deposit)
                } else {
                    continue;
                };

                let amount_minor = match parse_amount_minor(amount_str) {
                    Some(a) => a,
                    None => continue,
                };

                let reference_id = if ref_no.is_empty() {
                    extract_reference_id(&narration)
                } else {
                    Some(ref_no.to_string())
                };

                rows.push(StatementRow {
                    transaction_date: date,
                    merchant_raw: narration,
                    amount_minor,
                    currency: "INR".to_string(),
                    direction: direction.to_string(),
                    reference_id,
                    row_index,
                    llm_extracted: false,
                });
                row_index += 1;
            }
        }
    }
    rows
}

// ── ICICI Bank Account Parser ─────────────────────────────────────────────────
//
// Very similar to HDFC bank account; reuses the same logic with minor label differences.
fn parse_icici_account(pages: &[ParsedPage]) -> Vec<StatementRow> {
    // ICICI bank account has same column ordering as HDFC account
    parse_hdfc_account(pages)
}

// ── SBI Credit Card Parser ────────────────────────────────────────────────────
//
// SBI CC format:
//   Date: DD-MON-YYYY (e.g. 01-DEC-2023)
//   Columns: Sl No | Date | Description | Amount | Dr/Cr
//   Example: 1  01-DEC-2023  AMAZON INDIA  1500.00  Dr

fn parse_sbi_credit(pages: &[ParsedPage]) -> Vec<StatementRow> {
    let re_row = match Regex::new(
        r"(?xi)
        ^\d+\s+                               # serial number
        (\d{2}-[A-Z]{3}-\d{4})\s+            # DD-MON-YYYY date
        (.+?)\s+                               # description
        ([\d,]+\.\d{2})\s+                    # amount
        (Dr|Cr)\s*$                            # direction
        ",
    ) {
        Ok(r) => r,
        Err(_) => return vec![],
    };

    let mut rows = Vec::new();
    let mut row_index = 0usize;

    for page in pages {
        for line in page.text.lines() {
            let line = line.trim();
            if let Some(caps) = re_row.captures(line) {
                let date_raw = caps[1].to_uppercase();
                let desc = caps[2].trim().to_string();
                let amount_str = caps[3].trim();
                let dr_cr = caps[4].to_lowercase();

                if is_excluded_row(&desc) {
                    continue;
                }

                let date = match parse_date(&date_raw) {
                    Some(d) => d,
                    None => continue,
                };
                let amount_minor = match parse_amount_minor(amount_str) {
                    Some(a) => a,
                    None => continue,
                };
                let direction = if dr_cr == "dr" { "debit" } else { "credit" }.to_string();
                let reference_id = extract_reference_id(&desc);
                let merchant_raw = desc;

                rows.push(StatementRow {
                    transaction_date: date,
                    merchant_raw,
                    amount_minor,
                    currency: "INR".to_string(),
                    direction,
                    reference_id,
                    row_index,
                    llm_extracted: false,
                });
                row_index += 1;
            }
        }
    }
    rows
}

/// Issue #8: the row shape every Indian bank statement actually uses.
///
/// The seven bank-specific parsers above were each written against an
/// invented sample, and measurement against real statements showed none of
/// them matches what its bank issues:
///
/// ```text
/// Axis   27/06/2026  AIRTEL PAYMENTS BANK L,GURGAON   UTILITIES   588.82 Dr
/// IDFC   20/06/2026  ZEPTONOW 356 - Ref No: RT2617..  Retail       156.00 Dr
/// SBI    12 Jun 26   ASSPL      Bangalore   IN                     304.00  D
/// HDFC   14/04/2026| 00:00   10% Swiggy Cashback            +  C 30.30   l
/// ```
///
/// The differences are cosmetic — a trailing `Dr`/`Cr` versus a single
/// `D`/`C`, a leading `+` instead of a direction column, a two-digit year, a
/// `| HH:MM` time, a rupee glyph that decodes as `C` — over one common
/// structure: date, description, amount, and a marker saying which way the
/// money went. Handling that structure once beats maintaining seven
/// divergent guesses, so this runs whenever the bank-specific parser
/// recognises nothing.
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
                caps.name("amount").and_then(|m| parse_amount_minor(m.as_str())),
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

/// Recovers the description for a row that has none of its own.
///
/// HDFC and IDFC both wrap a long description *around* the line carrying the
/// date and amount, leaving that line with nothing between the two:
///
/// ```text
/// PAI INTERNATIONAL - Interest Amount
/// 25 Oct 25                        82.48 DR
/// Amortization - <3/3>
/// ```
///
/// The existing `merge_broken_line_boundaries` handles the opposite artifact
/// (a date alone, with everything else on the next line) and cannot see this
/// one. Three of nine real statements are laid out this way, and without
/// this every such row was dropped: the regex needs a description and there
/// is none on the line.
fn wrapped_description(lines: &[&str], is_row: &[bool], index: usize) -> String {
    let usable = |i: usize| -> Option<&str> {
        let candidate = lines.get(i)?;
        // A neighbouring row belongs to its own transaction, and a line
        // carrying its own amount is a total or a balance, not a
        // continuation.
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

/// Whether a continuation candidate is really a figure of its own.
fn looks_like_a_total(line: &str) -> bool {
    Regex::new(r"\d[\d,]*\.\d{2}")
        .map(|re| re.is_match(line))
        .unwrap_or(false)
}

/// Which way the money went, from whichever marker the bank happened to use.
/// Debit is the default: a purchase is what an unannotated row is on every
/// statement observed, and treating an unknown row as income would overstate
/// what the user earned.
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

/// Keeps only the description column, dropping whatever the bank prints
/// beside it.
///
/// Now that `statements::layout` preserves column geometry, the wide run of
/// spaces separating cells is real information. Real rows carry a merchant
/// category or a city in the next column —
/// `"ZEPTONOW 356 - Ref No: RT2617…          Retail Outlet Services"` and
/// `"ASSPL         Bangalore   IN"` — and keeping it would make
/// `merchant_display_name` the concatenation of the merchant and its
/// category, which is exactly the kind of junk name issue #12's cleanup pass
/// exists to repair. Better not to create it.
///
/// Falls back to the whole description when the first column is too thin to
/// be a merchant name on its own, so a row is never made worse than before.
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

/// Guards against matching a line that merely *looks* like a transaction.
/// A real statement contains rows such as
/// `15/06/2026 To 14/07/2026    Rs. 1,08,000.00` — a billing period beside a
/// credit limit, which satisfies date-description-amount perfectly and would
/// otherwise be imported as a ₹1,08,000 purchase from a merchant called "To".
fn has_enough_letters(description: &str) -> bool {
    description.chars().filter(|c| c.is_alphabetic()).count() >= 3
}

// ── Reference ID extraction ───────────────────────────────────────────────────

/// Extracts a reference ID (UTR/RRN) from a description string.
/// Heuristic: a 12-digit numeric sequence.
pub(crate) fn extract_reference_id(description: &str) -> Option<String> {
    let re = Regex::new(r"\b(\d{12})\b").ok()?;
    re.captures(description)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

// ── DB helpers: map rows → statement_entries → transaction_observations ───────

/// Maps extracted `StatementRow`s to `statement_entries` rows in SQLite.
/// Returns the list of new statement_entry IDs in the same order as `rows`.
///
/// Individual failures are logged and skipped — they do NOT abort the batch (Doc 10 §16).
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
                let entry_id = uuid::Uuid::new_v4().to_string();
                let ref_id = row.reference_id.clone();
                let result = c.execute(
                    "INSERT INTO statement_entries \
                     (id, statement_id, row_index, transaction_date, description_raw, \
                      merchant_raw, amount, amount_minor, currency, direction, reference_id, \
                      raw_row_json, created_at) \
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, '{}', datetime('now'))",
                    rusqlite::params![
                        entry_id,
                        stmt_id,
                        row.row_index as i64,
                        row.transaction_date,
                        row.merchant_raw,
                        row.merchant_raw, // merchant_raw = description_raw at this stage
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
                        // Skip this row; continue with rest (§16 invariant)
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

    // ── Issue #8: the row shapes real banks actually issue ────────────────
    //
    // Every case below is the literal line layout taken from a real
    // statement (values changed), after `statements::layout` rebuilds the
    // column geometry pdfium's `all()` destroys. Measured across nine real
    // statements, the seven bank-specific parsers above match *none* of
    // these and contribute zero rows; all of them come from
    // `parse_universal`.

    /// Axis and IDFC: a merchant-category column between the description and
    /// the amount, direction as a trailing `Dr`/`Cr`. `parse_axis_credit`
    /// expects a `+`/`-` prefixed amount and matches nothing here.
    #[test]
    fn real_axis_row_shape_is_parsed() {
        let pages = vec![make_page(
            "04/07/2026    AIRTEL PAYMENTS BANK L,GURGAON      UTILITIES     588.82 Dr\n\
             05/07/2026    AMAZON REFUND MUMBAI                RETAIL      1,200.00 Cr\n\
             01/07/2026    BBPS PAYMENT RECEIVED - PU016182HK9XJLQE9713    15,636.26 Cr\n",
        )];
        let rows = parse_universal(&pages);
        // Two rows, not three: the third line is the card bill being paid
        // via BBPS, which the exclusion list drops. Counting it would
        // double the payment already captured from the paying account.
        assert_eq!(rows.len(), 2, "got {rows:?}");
        assert_eq!(rows[0].transaction_date, "2026-07-04");
        assert_eq!(rows[0].amount_minor, 58_882);
        assert_eq!(rows[0].direction, "debit");
        // The category column is dropped — keeping it would make the
        // merchant name "AIRTEL PAYMENTS BANK L,GURGAON UTILITIES".
        assert_eq!(rows[0].merchant_raw, "AIRTEL PAYMENTS BANK L,GURGAON");
        assert_eq!(rows[1].direction, "credit");
        assert_eq!(rows[1].amount_minor, 120_000);
    }

    /// SBI: a two-digit year and a single-letter direction marker.
    #[test]
    fn real_sbi_row_shape_is_parsed() {
        let pages = vec![make_page(
            "12 Jun 26   ASSPL         Bangalore   IN            304.00  D\n\
             09 Jun 26   CARD CASHBACK CREDIT                    250.00  C\n",
        )];
        let rows = parse_universal(&pages);
        assert_eq!(rows.len(), 2, "got {rows:?}");
        // "12 Jun 26" must be 2026, not the year 26 — chrono's `%Y` accepts a
        // bare "26" and silently produced "0026-06-12" until the two-digit
        // formats were tried first.
        assert_eq!(rows[0].transaction_date, "2026-06-12");
        assert_eq!(rows[0].direction, "debit");
        assert_eq!(rows[1].direction, "credit");
    }

    /// HDFC: a `| HH:MM` time after the date, a rupee glyph that decodes as
    /// `C`, credits marked by a leading `+` with no direction column at all,
    /// and a trailing bullet glyph.
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

    /// HDFC and IDFC wrap a long description around the date/amount line,
    /// leaving nothing between the two. Three of nine real statements are
    /// laid out this way and every such row was dropped before.
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

    /// The guard against a line that merely looks like a transaction. Real
    /// statements print a billing period beside a credit limit, which
    /// satisfies date-description-amount perfectly and would import as a
    /// ₹1,08,000 purchase from a merchant called "To".
    #[test]
    fn a_billing_period_line_is_not_a_transaction() {
        let pages = vec![make_page(
            "15/06/2026 To 14/07/2026                        Rs. 1,08,000.00\n",
        )];
        assert!(parse_universal(&pages).is_empty());
    }

    /// The fallback must not displace a bank parser that does recognise its
    /// format — it only runs when nothing was found.
    #[test]
    fn the_universal_parser_only_runs_when_the_bank_parser_finds_nothing() {
        let recognised = "01/12/2023  SWIGGY ORDER BANGALORE     1,250.00  DR\n";
        let rows = extract_rows(&[make_page(recognised)], BankParser::HdfcCreditCard).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(
            !rows[0].llm_extracted,
            "a bank-parser row must not be tagged as fallback-extracted"
        );

        // The same page routed through a parser that cannot read it still
        // yields the row, via the fallback.
        let rows = extract_rows(&[make_page(recognised)], BankParser::Unknown).unwrap();
        assert_eq!(rows.len(), 1);
    }

    /// Two-digit and four-digit years must both round-trip, in every
    /// separator style the statements use.
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

    // ── test_hdfc_statement_row_parse ─────────────────────────────────────────

    #[test]
    fn test_hdfc_credit_statement_row_parse() {
        let text = "HDFC Bank Credit Card Statement\n\
                    01/12/2023  SWIGGY ORDER BANGALORE                    1,250.00  DR\n\
                    Opening Balance                                        50,000.00  CR\n\
                    15/12/2023  PAYMENT - THANK YOU                       20,000.00  CR\n\
                    31/12/2023  AMAZON INDIA MUMBAI 123456789012          3,500.00  DR\n\
                    Total Transactions                                    24,750.00\n";
        let pages = vec![make_page(text)];
        let rows = parse_hdfc_credit(&pages);

        // "Opening Balance" and "Total Transactions" are excluded as headers
        // and totals — and so is "PAYMENT - THANK YOU", which is the card
        // being paid off rather than a transaction. This test previously
        // expected three rows: `is_excluded_row` matched on raw substrings,
        // so the sibling AMEX case ("PAYMENT RECEIVED - THANK YOU") was
        // excluded while HDFC's punctuated wording of the same line was not.
        // Counting it would double the payment against the debit already
        // captured from the paying account's own statement.
        assert_eq!(
            rows.len(),
            2,
            "Must extract exactly 2 rows (excluding headers, totals and the card payment)"
        );

        let swiggy = &rows[0];
        assert_eq!(swiggy.transaction_date, "2023-12-01");
        assert_eq!(swiggy.merchant_raw, "SWIGGY ORDER BANGALORE");
        assert_eq!(swiggy.amount_minor, 125_000);
        assert_eq!(swiggy.direction, "debit");
        assert_eq!(swiggy.currency, "INR");

        let amazon = &rows[1];
        assert_eq!(amazon.reference_id, Some("123456789012".to_string()));
        assert_eq!(amazon.direction, "debit");
    }

    // ── test_icici_statement_row_parse ────────────────────────────────────────

    #[test]
    fn test_icici_credit_statement_row_parse() {
        let text = "ICICI Bank Platinum Credit Card\n\
                    01/12/2023  AMAZON INDIA NEW DELHI        2,500.00  -           2,500.00\n\
                    10/12/2023  SALARY CREDIT                 -          50,000.00  50,000.00\n\
                    Closing Balance                                                 47,500.00\n";
        let pages = vec![make_page(text)];
        let rows = parse_icici_credit(&pages);

        // Closing Balance must be excluded
        assert_eq!(rows.len(), 2, "Must extract 2 rows");

        let amazon = &rows[0];
        assert_eq!(amazon.transaction_date, "2023-12-01");
        assert_eq!(amazon.direction, "debit");
        assert_eq!(amazon.amount_minor, 250_000);

        let salary = &rows[1];
        assert_eq!(salary.direction, "credit");
        assert_eq!(salary.amount_minor, 5_000_000);
    }

    // ── test_amex_statement_row_parse ─────────────────────────────────────────

    #[test]
    fn test_amex_statement_row_parse() {
        let text = "American Express India Statement\n\
                    12/05/2023  SWIGGY ORDER                          1,300.00\n\
                    12/15/2023  PAYMENT RECEIVED - THANK YOU         -5,000.00\n\
                    Total Transactions                                -3,700.00\n";
        let pages = vec![make_page(text)];
        let rows = parse_amex_india(&pages);

        // "Total Transactions" must be excluded; PAYMENT RECEIVED should also be excluded
        // (contains "payment received")
        assert_eq!(rows.len(), 1, "Must extract 1 debit row");
        assert_eq!(rows[0].direction, "debit");
        assert_eq!(rows[0].amount_minor, 130_000);
        assert_eq!(rows[0].transaction_date, "2023-12-05"); // MM/DD/YYYY → YYYY-MM-DD
    }

    // ── test_broken_ocr_row_boundary_merged ──────────────────────────────────

    #[test]
    fn test_broken_ocr_row_boundary_merged() {
        // Doc 30 TASK-STMT-005: a common PDF-extraction rendering artifact —
        // the date lands alone on its own line, with description+amount+DR/CR
        // spilling onto the next. Without merging, HDFC's single-line regex
        // would never match this row at all (silently dropped, not merely
        // misparsed) — proving this end-to-end through the public
        // `extract_rows` entry point, not a merge helper in isolation.
        let text = "HDFC Bank Credit Card Statement\n\
                    01/12/2023\n\
                    SWIGGY ORDER BANGALORE                    1,250.00  DR\n\
                    15/12/2023  BIGBASKET BANGALORE                       2,000.00  DR\n";
        let pages = vec![make_page(text)];
        let rows = extract_rows(&pages, BankParser::HdfcCreditCard).unwrap();

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

    // ── test_balance_rows_excluded ────────────────────────────────────────────

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
            // Issue #8: the same phrases as the banks actually punctuate
            // them. Matching on raw substrings let "PAYMENT - THANK YOU"
            // through while "PAYMENT RECEIVED - THANK YOU" was caught.
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
        // Real transaction descriptions must NOT be excluded
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

    // ── Reference ID extraction ───────────────────────────────────────────────

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

    // ── SBI parser ────────────────────────────────────────────────────────────

    #[test]
    fn test_sbi_credit_row_parse() {
        let text = "SBI Credit Card Statement\n\
                    1  01-DEC-2023  AMAZON INDIA PURCHASE  1500.00  Dr\n\
                    2  15-DEC-2023  PAYMENT RECEIVED       5000.00  Cr\n\
                    Closing Balance                        3500.00\n";
        let pages = vec![make_page(text)];
        let rows = parse_sbi_credit(&pages);

        // Payment Received excluded; Closing Balance excluded → 1 row
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].direction, "debit");
        assert_eq!(rows[0].transaction_date, "2023-12-01");
        assert_eq!(rows[0].amount_minor, 150_000);
    }

    // ── test_axis_credit_statement_row_parse (Doc 30 TASK-STMT-005) ──────────
    // `parse_axis_credit` existed with zero test coverage before this task.

    #[test]
    fn test_axis_credit_statement_row_parse() {
        let text = "Axis Bank Credit Card Statement\n\
                    05/12/2023  NETFLIX INDIA MUMBAI               -649.00\n\
                    Opening Balance                                +50,000.00\n\
                    20/12/2023  SALARY REVERSAL                  +15,000.00\n";
        let pages = vec![make_page(text)];
        let rows = parse_axis_credit(&pages);

        assert_eq!(rows.len(), 2, "Opening Balance row must be excluded");

        let netflix = &rows[0];
        assert_eq!(netflix.transaction_date, "2023-12-05");
        assert_eq!(netflix.merchant_raw, "NETFLIX INDIA MUMBAI");
        assert_eq!(netflix.amount_minor, 64_900);
        assert_eq!(netflix.direction, "debit", "- prefix must map to debit");

        let reversal = &rows[1];
        assert_eq!(reversal.direction, "credit", "+ prefix must map to credit");
        assert_eq!(reversal.amount_minor, 1_500_000);
    }

    // ── test_hdfc_bank_account_row_parse (Doc 30 TASK-STMT-005) ──────────────
    // `parse_hdfc_account` existed with zero test coverage before this task —
    // the distinct bank-account layout (withdrawal/deposit/balance columns,
    // direction inferred from which amount column is populated) is a
    // different format from the credit-card parser above.

    #[test]
    fn test_hdfc_bank_account_row_parse() {
        let text = "HDFC Bank Savings Account Statement\n\
                    01/12/2023  UPI/SWIGGY/BANGALORE ref123 01/12/2023  2,500.00  -  47,500.00\n\
                    05/12/2023  NEFT SALARY CREDIT ref456 05/12/2023  -  50,000.00  97,500.00\n";
        let pages = vec![make_page(text)];
        let rows = parse_hdfc_account(&pages);

        assert_eq!(rows.len(), 2);

        let withdrawal = &rows[0];
        assert_eq!(withdrawal.transaction_date, "2023-12-01");
        assert_eq!(
            withdrawal.direction, "debit",
            "withdrawal column populated → debit"
        );
        assert_eq!(withdrawal.amount_minor, 250_000);

        let deposit = &rows[1];
        assert_eq!(
            deposit.direction, "credit",
            "deposit column populated → credit"
        );
        assert_eq!(deposit.amount_minor, 5_000_000);
    }

    // ── test_icici_bank_account_row_parse (Doc 30 TASK-STMT-005) ─────────────
    // `parse_icici_account` existed (aliased to the HDFC bank-account parser,
    // per its own doc comment — the two banks' common Indian-bank statement
    // template share this column layout) but had zero test coverage of its
    // own named entry point.

    #[test]
    fn test_icici_bank_account_row_parse() {
        let text = "ICICI Bank Savings Account Statement\n\
                    10/12/2023  UPI/AMAZON/DELHI ref789 10/12/2023  1,200.00  -  98,800.00\n";
        let pages = vec![make_page(text)];
        let rows = parse_icici_account(&pages);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].transaction_date, "2023-12-10");
        assert_eq!(rows[0].direction, "debit");
        assert_eq!(rows[0].amount_minor, 120_000);
    }
}
