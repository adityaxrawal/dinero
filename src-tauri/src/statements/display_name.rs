//! Derives a readable name for a statement.
//!
//! Prefers whatever the document itself reveals over the raw filename, which is
//! frequently an opaque reference number.
use crate::extraction::normalization::clean_masked_identifier;

#[derive(Debug, Default)]
pub struct StatementNameSource<'a> {
    pub filename: &'a str,
    pub sender: Option<&'a str>,
    pub subject: Option<&'a str>,
    pub snippet: Option<&'a str>,
    pub date: Option<&'a str>,
}

const MONTHS: [&str; 12] = [
    "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
];

/// Derives a readable name for a statement from whatever signals exist.
///
/// Combines bank, last four digits and period. Any part may be missing, so the
/// name is assembled from what was found rather than requiring all of it.
pub fn derive_display_name(source: &StatementNameSource<'_>) -> Option<String> {
    let bank = detect_bank_code(source)?;
    let (month, year) = detect_month_year(source)?;
    match detect_last_four(source) {
        Some(last4) => Some(format!("{bank}BANKXXXX{last4}{month}{year}")),
        None => Some(format!("{bank}BANK{month}{year}")),
    }
}

/// Identifies the bank from the document or its filename.
fn detect_bank_code(source: &StatementNameSource<'_>) -> Option<String> {
    let registry = crate::ingestion::verified_senders::SenderValidator::new();

    if let Some(sender) = source.sender {
        if let Some(name) = registry.short_name_for_sender(sender) {
            return Some(compact_bank_code(&name));
        }
    }

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

/// Compacts a bank name into a short code for display.
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

const MIN_ACCOUNT_DIGITS: usize = 10;

/// Finds the account's last four digits.
fn detect_last_four(source: &StatementNameSource<'_>) -> Option<String> {
    if let Some(found) = last_four_from_filename(source.filename) {
        return Some(found);
    }
    for text in [source.subject, source.snippet].into_iter().flatten() {
        if let Some(found) = last_four_from_text(text) {
            return Some(found);
        }
    }
    None
}

/// Extracts last-four digits from a filename.
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
        let run = longest_digit_run(token);
        if run.len() >= MIN_ACCOUNT_DIGITS && parse_filename_date(token).is_none() {
            return Some(clean_masked_identifier(run));
        }
    }
    None
}

/// Longest consecutive digit run in a token.
///
/// The chained sentinel is what closes a run ending at the token's end, avoiding
/// a separate post-loop case that is easy to get wrong.
fn longest_digit_run(token: &str) -> &str {
    let mut best = "";
    let mut start: Option<usize> = None;
    for (i, c) in token
        .char_indices()
        .chain(std::iter::once((token.len(), ' ')))
    {
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

/// Extracts last-four digits from document text, using surrounding labels.
fn last_four_from_text(text: &str) -> Option<String> {
    let re = regex::Regex::new(
        r"(?i)(?:ending(?:\s+in)?|card\s+(?:no\.?|number)?|a/c\s+(?:no\.?)?|account\s+(?:no\.?)?)\s*[Xx*\s\-]*(\d{2,4})\b",
    )
    .ok()?;
    let captured = re.captures(text)?.get(1)?.as_str();
    if is_plausible_year(captured) {
        return None;
    }
    Some(captured.to_string())
}

/// Whether a four-digit string is more plausibly a year than an account number.
///
/// Prevents a statement period year being recorded as the account's last four
/// digits, which would then mis-key instrument attribution.
fn is_plausible_year(digits: &str) -> bool {
    digits.len() == 4
        && digits
            .parse::<u32>()
            .map(|n| (2000..=2100).contains(&n))
            .unwrap_or(false)
}

/// Determines the statement's month and year.
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

/// Parses a date embedded in a filename token.
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

/// Parses an eight-digit ddmmyyyy date.
fn from_ddmmyyyy(digits: &str) -> Option<(String, u32)> {
    let day: u32 = digits.get(0..2)?.parse().ok()?;
    let month: usize = digits.get(2..4)?.parse().ok()?;
    let year: u32 = digits.get(4..8)?.parse().ok()?;
    if !(1..=31).contains(&day) || !(1..=12).contains(&month) || !(2000..=2100).contains(&year) {
        return None;
    }
    Some((MONTHS[month - 1].to_string(), year))
}

/// Parses the date from an email header.
fn parse_email_date(raw: &str) -> Option<(String, u32)> {
    let parsed = chrono::DateTime::parse_from_rfc2822(raw.trim())
        .map(|d| d.naive_utc().date())
        .or_else(|_| chrono::DateTime::parse_from_rfc3339(raw.trim()).map(|d| d.naive_utc().date()))
        .ok()?;
    use chrono::Datelike;
    Some((
        MONTHS[(parsed.month() - 1) as usize].to_string(),
        parsed.year() as u32,
    ))
}

/// Splits a filename into tokens on its separators.
fn filename_tokens(filename: &str) -> Vec<&str> {
    let stem = filename
        .rsplit_once('.')
        .map(|(s, _)| s)
        .unwrap_or(filename);
    stem.split(['_', ' ']).filter(|t| !t.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn a_real_attachment_filename_alone_is_enough() {
        let name = derive_display_name(&StatementNameSource {
            filename: "5268XXXXXXXXXX64_13-05-2026_315.pdf",
            subject: Some("HDFC Bank statement"),
            ..Default::default()
        });
        assert_eq!(name.as_deref(), Some("HDFCBANKXXXX64MAY2026"));
    }

    #[test]
    fn a_concatenated_period_resolves_to_the_closing_month() {
        assert_eq!(
            parse_filename_date("2301202622022026"),
            Some(("FEB".to_string(), 2026))
        );
    }

    #[test]
    fn a_long_card_number_is_not_read_as_a_date() {
        assert_eq!(parse_filename_date("8798828479959148"), None);
        assert_eq!(
            last_four_from_filename("8798828479959148_09072026.pdf").as_deref(),
            Some("9148")
        );
    }

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

    #[test]
    fn bank_suffix_is_not_repeated() {
        assert_eq!(compact_bank_code("HDFCBank"), "HDFC");
        assert_eq!(compact_bank_code("Yes Bank"), "YES");
        assert_eq!(compact_bank_code("SBI"), "SBI");
        assert_eq!(compact_bank_code("IDFC FIRST Bank"), "IDFCFIRST");
    }

    #[test]
    fn an_unknown_bank_produces_no_name() {
        let name = derive_display_name(&StatementNameSource {
            filename: "scan001.pdf",
            subject: Some("here is the document"),
            ..Default::default()
        });
        assert_eq!(name, None);
    }

    #[test]
    fn a_year_is_not_mistaken_for_a_card_tail() {
        assert_eq!(last_four_from_text("Statement for account no. 2025"), None);
        assert_eq!(
            last_four_from_text("card ending 4321 for 2026").as_deref(),
            Some("4321")
        );
    }

    #[test]
    fn a_filename_with_no_card_number_omits_the_mask() {
        let name = derive_display_name(&StatementNameSource {
            filename: "CC_STMT_749341660_347480_2301202622022026.pdf",
            sender: Some("Emailstatements.cards@hdfcbank.net"),
            ..Default::default()
        });
        assert_eq!(name.as_deref(), Some("HDFCBANKFEB2026"));
    }

    #[test]
    fn every_real_filename_is_handled() {
        let expected: [(&str, Option<&str>); 7] = [
            (
                "20000007937556_21112025_211211204.pdf",
                Some("HDFCBANKXXXX7556NOV2025"),
            ),
            (
                "5268XXXXXXXXXX64_13-05-2026_315.pdf",
                Some("HDFCBANKXXXX64MAY2026"),
            ),
            (
                "5372XXXXXXXXXX83_14-04-2026_360.pdf",
                Some("HDFCBANKXXXX83APR2026"),
            ),
            ("560103_1005210000701522-246.pdf", None),
            (
                "6529XXXXXXXXXX56_01-05-2026_616.pdf",
                Some("HDFCBANKXXXX56MAY2026"),
            ),
            (
                "8798828479959148_09072026.pdf",
                Some("HDFCBANKXXXX9148JUL2026"),
            ),
            (
                "CC_STMT_749341660_347480_2301202622022026.pdf",
                Some("HDFCBANKFEB2026"),
            ),
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
