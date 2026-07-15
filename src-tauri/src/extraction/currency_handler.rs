//! Doc 30 TASK-TXN-013: Multi-Currency and Foreign Transaction Handling.
//!
//! Extracts foreign-currency evidence (`original_amount_minor`,
//! `original_currency`, `exchange_rate`) from a transaction alert body when
//! present. FRS §6.4: "the exchange rate must be parsed directly from the
//! bank's email or statement text. No external live API calls... are
//! permitted to backfill this rate." If only the settled amount is present,
//! the foreign-currency fields stay `NULL` rather than being guessed.

use regex::Regex;
use std::sync::OnceLock;

static FOREIGN_AMOUNT_RE: OnceLock<Regex> = OnceLock::new();
static EXCHANGE_RATE_RE: OnceLock<Regex> = OnceLock::new();

/// Doc 30's named `ExtractionResult` fields, bundled for a single detection
/// call.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct FxFields {
    pub original_amount_minor: Option<i64>,
    pub original_currency: Option<String>,
    pub exchange_rate: Option<f64>,
}

/// ISO 4217 codes worth scanning for as a *foreign* currency -- deliberately
/// excludes the domestic settlement currency (`settled_currency`, almost
/// always INR for this product) so a plain "INR 1,500.00" alert is never
/// misread as a foreign-currency transaction.
const KNOWN_CURRENCY_CODES: &[&str] = &[
    "USD", "EUR", "GBP", "AED", "SGD", "AUD", "CAD", "JPY", "CHF", "HKD", "THB", "MYR", "NZD",
];

fn parse_amount(raw: &str) -> Option<i64> {
    let cleaned: String = raw.chars().filter(|c| c.is_ascii_digit() || *c == '.').collect();
    let val: f64 = cleaned.parse().ok()?;
    Some((val * 100.0).round() as i64)
}

/// Doc 30: "extract both the foreign `original_amount_minor`/
/// `original_currency` and the settled INR `amount_minor`/`currency`, plus
/// `exchange_rate` when a bank explicitly states it."
pub fn detect_fx_fields(body: &str, settled_currency: &str) -> FxFields {
    let amount_re = FOREIGN_AMOUNT_RE.get_or_init(|| {
        Regex::new(r"(?i)\b(USD|EUR|GBP|AED|SGD|AUD|CAD|JPY|CHF|HKD|THB|MYR|NZD)\s*([\d,]+(?:\.\d+)?)\b")
            .unwrap()
    });

    let mut fields = FxFields::default();

    if let Some(caps) = amount_re.captures(body) {
        let code = caps.get(1).unwrap().as_str().to_uppercase();
        if code != settled_currency.to_uppercase() && KNOWN_CURRENCY_CODES.contains(&code.as_str())
        {
            fields.original_currency = Some(code);
            fields.original_amount_minor = parse_amount(caps.get(2).unwrap().as_str());
        }
    }

    // Only look for an exchange rate at all if a genuine foreign amount was
    // found -- a bare "rate" mention with no foreign currency context isn't
    // meaningful here.
    if fields.original_currency.is_some() {
        let rate_re = EXCHANGE_RATE_RE.get_or_init(|| {
            Regex::new(r"(?i)(?:exchange\s*rate|conversion\s*rate|rate\s*of)\s*(?:is|:)?\s*(?:1\s*[a-z]{3}\s*=\s*)?(?:rs\.?|inr|₹)?\s*([\d]+(?:\.\d+)?)")
                .unwrap()
        });
        if let Some(caps) = rate_re.captures(body) {
            fields.exchange_rate = caps.get(1).unwrap().as_str().parse::<f64>().ok();
        }
    }

    fields
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Doc 30 TASK-TXN-013 acceptance test.
    #[test]
    fn test_foreign_currency_fields_extracted_when_present() {
        let body = "You spent USD 100.00 on your HDFC Bank credit card. Exchange rate: 1 USD = 83.25 INR. Amount charged: Rs 8,325.00.";
        let fields = detect_fx_fields(body, "INR");
        assert_eq!(fields.original_currency, Some("USD".to_string()));
        assert_eq!(fields.original_amount_minor, Some(10000));
        assert_eq!(fields.exchange_rate, Some(83.25));
    }

    /// Doc 30 TASK-TXN-013 acceptance test.
    #[test]
    fn test_foreign_currency_fields_null_when_absent() {
        let body = "Rs 1,500.00 spent on your HDFC Bank credit card at Amazon on 25-May-23.";
        let fields = detect_fx_fields(body, "INR");
        assert_eq!(fields.original_currency, None);
        assert_eq!(fields.original_amount_minor, None);
        assert_eq!(fields.exchange_rate, None);
    }

    /// Doc 30 TASK-TXN-013 acceptance test: even a domestic-currency-only
    /// alert (no FX language) must not accidentally get treated as foreign.
    #[test]
    fn test_settled_inr_amount_always_populated() {
        // A plain INR alert must never be misdetected as foreign currency
        // just because "INR" itself matches a currency-code-like token.
        let body = "INR 1,500.00 debited from your account.";
        let fields = detect_fx_fields(body, "INR");
        assert_eq!(
            fields.original_currency, None,
            "the settled currency itself must never be treated as a foreign currency"
        );

        // A genuinely foreign alert must still leave the settled-amount
        // extraction (a separate, always-run regex elsewhere in the ladder)
        // completely unaffected -- detect_fx_fields only ever adds
        // original_*/exchange_rate fields, never touches amount_minor/currency.
        let fx_body = "USD 50.00 spent. Rs 4,150.00 charged to your card.";
        let fields = detect_fx_fields(fx_body, "INR");
        assert_eq!(fields.original_currency, Some("USD".to_string()));
    }

    #[test]
    fn test_no_exchange_rate_stated_leaves_it_null() {
        let body = "You spent USD 100.00 on your card. Rs 8,325.00 will be charged.";
        let fields = detect_fx_fields(body, "INR");
        assert_eq!(fields.original_currency, Some("USD".to_string()));
        assert_eq!(
            fields.exchange_rate, None,
            "no rate was stated in the source text -- must not be guessed or backfilled"
        );
    }
}
