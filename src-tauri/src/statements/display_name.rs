//! Issue #9: a consistent, human-readable name for a statement sitting in the
//! Action Needed queue — `<BANK>BANKXXXX<LAST4><MON><YYYY>`, e.g.
//! `HDFCBANKXXXX1234JUN2026`.
//!
//! ## Why this is in Rust
//!
//! This previously lived in `src/lib/formatStatementName.ts`, which carried
//! its own hardcoded list of fourteen bank names and its own last-four regex.
//! Both already existed on this side and better: `verified_senders_registry`
//! knows 225 sender domains across every bank the app ingests from, and
//! `clean_masked_identifier` (issue #11) already handles every masking shape
//! the banks use. Two divergent copies of the same knowledge is how a name
//! ends up correct in the queue and wrong everywhere else.
//!
//! ## Why it is computed on read
//!
//! Derived in `group_unprocessed_by_status` from the fields already persisted
//! in `unprocessed_statements.statement_source_json`, rather than written at
//! row-creation time. Rows created before this existed then get a proper name
//! too, with no migration and no backfill — and a manual upload, which has no
//! email context at all, is handled by the same code path.
//!
//! ## Source precedence
//!
//! The attachment filename is consulted first and is usually decisive: banks
//! name the file after the card and the statement date
//! (`5268XXXXXXXXXX64_13-05-2026_315.pdf`). The email's sender domain, subject
//! and received date fill in whatever the filename does not carry.

use crate::extraction::normalization::clean_masked_identifier;

/// Everything known about a statement that never got parsed. All fields are
/// optional except the filename, because the pipeline records them
/// opportunistically — a manual upload has a filename and nothing else.
#[derive(Debug, Default)]
pub struct StatementNameSource<'a> {
    pub filename: &'a str,
    pub sender: Option<&'a str>,
    pub subject: Option<&'a str>,
    pub snippet: Option<&'a str>,
    /// The email's `Date` header, in whatever form Gmail supplied it.
    pub date: Option<&'a str>,
}

const MONTHS: [&str; 12] = [
    "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
];

/// Builds the display name, or returns `None` when too little is known to
/// produce anything better than the raw filename (the caller falls back to
/// it). A name is only worth showing if the bank is known — `UNKNOWNBANKXXXX`
/// tells the user strictly less than the filename the bank chose.
pub fn derive_display_name(source: &StatementNameSource<'_>) -> Option<String> {
    let bank = detect_bank_code(source)?;
    let (month, year) = detect_month_year(source)?;
    // The `XXXX` in the format is the mask standing in front of the real
    // digits. With no digits to mask it says nothing, and `…BANKXXXXXXXXFEB`
    // reads as a fault rather than as missing information — so the whole
    // segment is dropped instead.
    match detect_last_four(source) {
        Some(last4) => Some(format!("{bank}BANKXXXX{last4}{month}{year}")),
        None => Some(format!("{bank}BANK{month}{year}")),
    }
}

// ── Bank ──────────────────────────────────────────────────────────────────

/// Resolves the issuer to a compact code. The sender domain is authoritative
/// when present — an exact registry match beats scanning free text, where
/// "HDFC" in a forwarded quote or a marketing footer would mislabel the row.
fn detect_bank_code(source: &StatementNameSource<'_>) -> Option<String> {
    let registry = crate::ingestion::verified_senders::SenderValidator::new();

    if let Some(sender) = source.sender {
        if let Some(name) = registry.short_name_for_sender(sender) {
            return Some(compact_bank_code(&name));
        }
    }

    // No recognised domain (a forwarded statement, or a manual upload):
    // fall back to matching a known bank's name in the text we do have.
    // Longest display name first, so "IDFC FIRST Bank" is not shadowed by a
    // bank whose name is a prefix of it.
    let haystack = format!(
        "{} {} {}",
        source.subject.unwrap_or(""),
        source.snippet.unwrap_or(""),
        source.filename
    )
    .to_uppercase();

    let mut names = registry.all_display_names();
    names.sort_by_key(|n| std::cmp::Reverse(n.len()));
    names
        .into_iter()
        .find(|name| name.len() >= 3 && haystack.contains(&name.to_uppercase()))
        .map(|name| compact_bank_code(&name))
}

/// Squeezes a bank's name into the code that precedes `BANK` in the result.
/// A trailing "BANK" is dropped because the format supplies its own, so
/// "HDFCBank" becomes `HDFC` and yields `HDFCBANKXXXX…` rather than
/// `HDFCBANKBANKXXXX…`.
fn compact_bank_code(name: &str) -> String {
    let compact: String = name
        .to_uppercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    compact
        .strip_suffix("BANK")
        .filter(|s| !s.is_empty())
        .unwrap_or(&compact)
        .to_string()
}

// ── Last four ─────────────────────────────────────────────────────────────

/// Digits below this length in a filename token are reference numbers, batch
/// ids and branch codes, not card or account numbers. Real ones are 11–19
/// digits; `CC_STMT_749341660_347480_…` carries two shorter numbers that are
/// neither.
const MIN_ACCOUNT_DIGITS: usize = 10;

fn detect_last_four(source: &StatementNameSource<'_>) -> Option<String> {
    if let Some(found) = last_four_from_filename(source.filename) {
        return Some(found);
    }
    // The subject line is the next most reliable: banks write "card ending
    // 1234" there. The snippet is searched last, being ordinary prose.
    for text in [source.subject, source.snippet].into_iter().flatten() {
        if let Some(found) = last_four_from_text(text) {
            return Some(found);
        }
    }
    None
}

fn last_four_from_filename(filename: &str) -> Option<String> {
    for token in filename_tokens(filename) {
        let has_mask = token.contains(['X', 'x', '*']);
        if has_mask {
            let cleaned = clean_masked_identifier(token);
            if !cleaned.is_empty() && cleaned.chars().all(|c| c.is_ascii_digit()) {
                return Some(cleaned);
            }
            continue;
        }
        // The *longest consecutive run* of digits, not every digit in the
        // token: `1005210000701522-246` is an account number followed by a
        // three-digit suffix, and concatenating both before taking the last
        // four yields "2246", a number that is in neither.
        let run = longest_digit_run(token);
        // Long enough not to be a reference code — but not if it is really a
        // date, which `detect_month_year` claims from the same filename.
        if run.len() >= MIN_ACCOUNT_DIGITS && parse_filename_date(token).is_none() {
            return Some(clean_masked_identifier(run));
        }
    }
    None
}

fn longest_digit_run(token: &str) -> &str {
    let mut best = "";
    let mut start: Option<usize> = None;
    for (i, c) in token.char_indices().chain(std::iter::once((token.len(), ' '))) {
        match (c.is_ascii_digit(), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                if i - s > best.len() {
                    best = &token[s..i];
                }
                start = None;
            }
            _ => {}
        }
    }
    best
}

fn last_four_from_text(text: &str) -> Option<String> {
    let re = regex::Regex::new(
        r"(?i)(?:ending(?:\s+in)?|card\s+(?:no\.?|number)?|a/c\s+(?:no\.?)?|account\s+(?:no\.?)?)\s*[Xx*\s\-]*(\d{2,4})\b",
    )
    .ok()?;
    let captured = re.captures(text)?.get(1)?.as_str();
    // A four-digit "year" caught by the pattern is not a card tail.
    if is_plausible_year(captured) {
        return None;
    }
    Some(captured.to_string())
}

fn is_plausible_year(digits: &str) -> bool {
    digits.len() == 4
        && digits
            .parse::<u32>()
            .map(|n| (2000..=2100).contains(&n))
            .unwrap_or(false)
}

// ── Month and year ────────────────────────────────────────────────────────

fn detect_month_year(source: &StatementNameSource<'_>) -> Option<(String, u32)> {
    for token in filename_tokens(source.filename) {
        if let Some((month, year)) = parse_filename_date(token) {
            return Some((month, year));
        }
    }
    if let Some(date) = source.date {
        if let Some(found) = parse_email_date(date) {
            return Some(found);
        }
    }
    // Last resort: a month name and a year written into the subject, which is
    // how the covering email usually titles itself ("… Statement -
    // December-2025").
    let text = format!(
        "{} {}",
        source.subject.unwrap_or(""),
        source.snippet.unwrap_or("")
    )
    .to_uppercase();
    let month = MONTHS.iter().find(|m| text.contains(**m))?;
    let year = regex::Regex::new(r"\b(20\d{2})\b")
        .ok()?
        .captures(&text)?
        .get(1)?
        .as_str()
        .parse::<u32>()
        .ok()?;
    Some((month.to_string(), year))
}

/// Recognises the date forms banks put in attachment filenames:
/// `13-05-2026`, `21112025` (DDMMYYYY), and the concatenated period
/// `2301202622022026` (DDMMYYYY + DDMMYYYY), whose second half is the
/// statement's closing date.
fn parse_filename_date(token: &str) -> Option<(String, u32)> {
    let digits: String = token.chars().filter(|c| c.is_ascii_digit()).collect();

    if token.len() == 10 && token.matches(['-', '/']).count() == 2 && digits.len() == 8 {
        return from_ddmmyyyy(&digits);
    }
    if token.chars().all(|c| c.is_ascii_digit()) {
        if digits.len() == 8 {
            return from_ddmmyyyy(&digits);
        }
        if digits.len() == 16 {
            return from_ddmmyyyy(&digits[8..]);
        }
    }
    None
}

fn from_ddmmyyyy(digits: &str) -> Option<(String, u32)> {
    let day: u32 = digits.get(0..2)?.parse().ok()?;
    let month: usize = digits.get(2..4)?.parse().ok()?;
    let year: u32 = digits.get(4..8)?.parse().ok()?;
    if !(1..=31).contains(&day) || !(1..=12).contains(&month) || !(2000..=2100).contains(&year) {
        return None;
    }
    Some((MONTHS[month - 1].to_string(), year))
}

fn parse_email_date(raw: &str) -> Option<(String, u32)> {
    let parsed = chrono::DateTime::parse_from_rfc2822(raw.trim())
        .map(|d| d.naive_utc().date())
        .or_else(|_| {
            chrono::DateTime::parse_from_rfc3339(raw.trim()).map(|d| d.naive_utc().date())
        })
        .ok()?;
    use chrono::Datelike;
    Some((
        MONTHS[(parsed.month() - 1) as usize].to_string(),
        parsed.year() as u32,
    ))
}

/// Splits a filename into the fields banks separate with `_`, dropping the
/// extension. Hyphens are *not* separators — `13-05-2026` is one field.
fn filename_tokens(filename: &str) -> Vec<&str> {
    let stem = filename.rsplit_once('.').map(|(s, _)| s).unwrap_or(filename);
    stem.split(['_', ' '])
        .filter(|t| !t.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact shape issue #9 asks for.
    #[test]
    fn produces_the_specified_format() {
        let name = derive_display_name(&StatementNameSource {
            filename: "statement.pdf",
            sender: Some("Emailstatements.cards@hdfcbank.net"),
            subject: Some("Your HDFC Bank Credit Card Statement - card ending 1234"),
            date: Some("Sun, 14 Jun 2026 16:19:00 +0530"),
            ..Default::default()
        });
        assert_eq!(name.as_deref(), Some("HDFCBANKXXXX1234JUN2026"));
    }

    /// Real filenames from the corpus carry the card and the statement date,
    /// so a locked PDF names itself correctly even with no email context at
    /// all — the manual-upload case.
    #[test]
    fn a_real_attachment_filename_alone_is_enough() {
        let name = derive_display_name(&StatementNameSource {
            filename: "5268XXXXXXXXXX64_13-05-2026_315.pdf",
            subject: Some("HDFC Bank statement"),
            ..Default::default()
        });
        // HDFC masks all but two trailing digits; inventing two more would be
        // a lie, so the real tail is shown as-is.
        assert_eq!(name.as_deref(), Some("HDFCBANKXXXX64MAY2026"));
    }

    /// A filename whose date field is the billing *period* — two DDMMYYYY
    /// runs concatenated — resolves to the closing date, not the opening one.
    #[test]
    fn a_concatenated_period_resolves_to_the_closing_month() {
        assert_eq!(
            parse_filename_date("2301202622022026"),
            Some(("FEB".to_string(), 2026))
        );
    }

    /// A 16-digit token that is a card number, not a date, must not be
    /// mistaken for one: its trailing 8 digits parse to day 79.
    #[test]
    fn a_long_card_number_is_not_read_as_a_date() {
        assert_eq!(parse_filename_date("8798828479959148"), None);
        assert_eq!(
            last_four_from_filename("8798828479959148_09072026.pdf").as_deref(),
            Some("9148")
        );
    }

    /// Short numeric fields in a filename are references and branch codes,
    /// never the account — picking one would put a meaningless number in
    /// front of the user.
    #[test]
    fn short_reference_numbers_are_not_treated_as_accounts() {
        assert_eq!(
            last_four_from_filename("CC_STMT_749341660_347480_2301202622022026.pdf").as_deref(),
            None
        );
        assert_eq!(
            last_four_from_filename("560103_1005210000701522-246.pdf").as_deref(),
            Some("1522")
        );
    }

    /// The sender domain outranks free text: a bank named in a quoted reply
    /// or a marketing footer must not override who actually sent the mail.
    #[test]
    fn sender_domain_outranks_text_mentions() {
        let code = detect_bank_code(&StatementNameSource {
            filename: "stmt.pdf",
            sender: Some("cc.statements@axisbank.com"),
            subject: Some("Fwd: your ICICI Bank and HDFC Bank statements"),
            ..Default::default()
        });
        assert_eq!(code.as_deref(), Some("AXIS"));
    }

    /// The format appends its own "BANK", so a name already ending in it must
    /// not double up.
    #[test]
    fn bank_suffix_is_not_repeated() {
        assert_eq!(compact_bank_code("HDFCBank"), "HDFC");
        assert_eq!(compact_bank_code("Yes Bank"), "YES");
        assert_eq!(compact_bank_code("SBI"), "SBI");
        assert_eq!(compact_bank_code("IDFC FIRST Bank"), "IDFCFIRST");
    }

    /// An unidentifiable bank yields nothing, so the caller shows the
    /// filename — which at least came from the bank — instead of a row
    /// labelled `UNKNOWNBANKXXXXXXXX`.
    #[test]
    fn an_unknown_bank_produces_no_name() {
        let name = derive_display_name(&StatementNameSource {
            filename: "scan001.pdf",
            subject: Some("here is the document"),
            ..Default::default()
        });
        assert_eq!(name, None);
    }

    /// A year in the subject must not be harvested as a card tail.
    #[test]
    fn a_year_is_not_mistaken_for_a_card_tail() {
        assert_eq!(last_four_from_text("Statement for account no. 2025"), None);
        assert_eq!(
            last_four_from_text("card ending 4321 for 2026").as_deref(),
            Some("4321")
        );
    }

    /// A statement whose filename carries no card number at all still gets a
    /// usable name — just without the masked segment, rather than a run of
    /// eight X's that looks like a bug.
    #[test]
    fn a_filename_with_no_card_number_omits_the_mask() {
        let name = derive_display_name(&StatementNameSource {
            filename: "CC_STMT_749341660_347480_2301202622022026.pdf",
            sender: Some("Emailstatements.cards@hdfcbank.net"),
            ..Default::default()
        });
        assert_eq!(name.as_deref(), Some("HDFCBANKFEB2026"));
    }

    /// Every real filename supplied for testing resolves to a well-formed
    /// name: no panics, no stray mask runs, no missing month.
    #[test]
    fn every_real_filename_is_handled() {
        // `560103_1005210000701522-246` carries no date anywhere in its name,
        // so with no email context there is no month to report — declined
        // rather than guessed, and the caller shows the filename instead.
        let expected: [(&str, Option<&str>); 7] = [
            ("20000007937556_21112025_211211204.pdf", Some("HDFCBANKXXXX7556NOV2025")),
            ("5268XXXXXXXXXX64_13-05-2026_315.pdf", Some("HDFCBANKXXXX64MAY2026")),
            ("5372XXXXXXXXXX83_14-04-2026_360.pdf", Some("HDFCBANKXXXX83APR2026")),
            ("560103_1005210000701522-246.pdf", None),
            ("6529XXXXXXXXXX56_01-05-2026_616.pdf", Some("HDFCBANKXXXX56MAY2026")),
            ("8798828479959148_09072026.pdf", Some("HDFCBANKXXXX9148JUL2026")),
            ("CC_STMT_749341660_347480_2301202622022026.pdf", Some("HDFCBANKFEB2026")),
        ];
        for (filename, want) in expected {
            let name = derive_display_name(&StatementNameSource {
                filename,
                sender: Some("Emailstatements.cards@hdfcbank.net"),
                ..Default::default()
            });
            assert_eq!(name.as_deref(), want, "for {filename}");
        }
    }
}
