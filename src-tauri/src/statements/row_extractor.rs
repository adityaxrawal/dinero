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

    let rows: Vec<StatementRow> = match parser {
        BankParser::HdfcCreditCard => parse_hdfc_credit(pages),
        BankParser::IciciCreditCard => parse_icici_credit(pages),
        BankParser::AxisCreditCard => parse_axis_credit(pages),
        BankParser::AmexIndia => parse_amex_india(pages),
        BankParser::HdfcBankAccount => parse_hdfc_account(pages),
        BankParser::IciciBankAccount => parse_icici_account(pages),
        BankParser::SbiCreditCard => parse_sbi_credit(pages),
        BankParser::Unknown => parse_generic_fallback(pages),
    };

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
fn is_excluded_row(description: &str) -> bool {
    let d = description.to_lowercase();
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
fn parse_date(s: &str) -> Option<String> {
    use chrono::NaiveDate;
    let s = s.trim();
    let formats = ["%d/%m/%Y", "%d-%m-%Y", "%d %b %Y", "%d-%b-%Y", "%Y-%m-%d"];
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
fn parse_amount_minor(s: &str) -> Option<i64> {
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

/// Generic fallback for unsupported banks (OCR/LLM-assist path).
/// Rows extracted here are tagged `llm_extracted = true` → `extraction_method = llm_assist`
/// → routed to ambiguous reconciliation clusters, not auto-merged (Doc 10 §11.3).
fn parse_generic_fallback(pages: &[ParsedPage]) -> Vec<StatementRow> {
    tracing::warn!("Using generic OCR/LLM-assist fallback — bank format not supported in v1.0");
    // Generic regex: date + description + amount on any line
    let re_row = match Regex::new(
        r"(?x)
        ^(\d{2}[/\-]\d{2}[/\-]\d{4})\s+   # date
        (.+?)\s+                              # description
        ([\d,]+\.\d{2})\s*$                 # amount
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
                let amount_str = caps[3].trim();

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

                let reference_id = extract_reference_id(&desc);
                let merchant_raw = desc;

                rows.push(StatementRow {
                    transaction_date: date,
                    merchant_raw,
                    amount_minor,
                    currency: "INR".to_string(),
                    direction: "debit".to_string(), // unknown — default to debit
                    reference_id,
                    row_index,
                    llm_extracted: true, // tag as LLM-assist path
                });
                row_index += 1;
            }
        }
    }
    rows
}

// ── Reference ID extraction ───────────────────────────────────────────────────

/// Extracts a reference ID (UTR/RRN) from a description string.
/// Heuristic: a 12-digit numeric sequence.
fn extract_reference_id(description: &str) -> Option<String> {
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

        // "Opening Balance" and "Total Transactions" must be excluded
        assert_eq!(
            rows.len(),
            3,
            "Must extract exactly 3 rows (excluding headers and totals)"
        );

        let swiggy = &rows[0];
        assert_eq!(swiggy.transaction_date, "2023-12-01");
        assert_eq!(swiggy.merchant_raw, "SWIGGY ORDER BANGALORE");
        assert_eq!(swiggy.amount_minor, 125_000);
        assert_eq!(swiggy.direction, "debit");
        assert_eq!(swiggy.currency, "INR");

        let payment = &rows[1];
        assert_eq!(payment.direction, "credit");
        assert_eq!(payment.amount_minor, 2_000_000);

        let amazon = &rows[2];
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
                    15/12/2023  PAYMENT - THANK YOU                       20,000.00  CR\n";
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
