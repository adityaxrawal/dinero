//! Detects foreign-currency fields on a transaction.
//!
//! A foreign charge carries both the original amount and the settled home-currency
//! amount, and conflating the two would misstate spending. This recovers the
//! original amount, its currency and the exchange rate so both are recorded.
use regex::Regex;
use std::sync::OnceLock;

static FOREIGN_AMOUNT_RE: OnceLock<Regex> = OnceLock::new();
static EXCHANGE_RATE_RE: OnceLock<Regex> = OnceLock::new();

#[derive(Debug, Default, Clone, PartialEq)]
pub struct FxFields {
    pub original_amount_minor: Option<i64>,
    pub original_currency: Option<String>,
    pub exchange_rate: Option<f64>,
}

const KNOWN_CURRENCY_CODES: &[&str] = &[
    "USD", "EUR", "GBP", "AED", "SGD", "AUD", "CAD", "JPY", "CHF", "HKD", "THB", "MYR", "NZD",
];

/// Parses an amount string into integer minor units.
///
/// Returns None rather than a default on failure -- a foreign amount that cannot
/// be parsed must be recorded as absent, since a wrong figure here would misstate
/// what the user actually spent abroad.
fn parse_amount(raw: &str) -> Option<i64> {
    let cleaned: String = raw
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let val: f64 = cleaned.parse().ok()?;
    Some((val * 100.0).round() as i64)
}

/// Detects the original currency, amount and rate on a foreign transaction.
///
/// A foreign charge carries two amounts: what was billed abroad and what was
/// settled at home. Conflating them would misstate spending, so both are
/// recovered and the settled currency is passed in to distinguish which is which.
pub fn detect_fx_fields(body: &str, settled_currency: &str) -> FxFields {
    let amount_re = FOREIGN_AMOUNT_RE.get_or_init(|| {
        Regex::new(
            r"(?i)\b(USD|EUR|GBP|AED|SGD|AUD|CAD|JPY|CHF|HKD|THB|MYR|NZD)\s*([\d,]+(?:\.\d+)?)\b",
        )
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

    #[test]
    fn test_foreign_currency_fields_extracted_when_present() {
        let body = "You spent USD 100.00 on your HDFC Bank credit card. Exchange rate: 1 USD = 83.25 INR. Amount charged: Rs 8,325.00.";
        let fields = detect_fx_fields(body, "INR");
        assert_eq!(fields.original_currency, Some("USD".to_string()));
        assert_eq!(fields.original_amount_minor, Some(10000));
        assert_eq!(fields.exchange_rate, Some(83.25));
    }

    #[test]
    fn test_foreign_currency_fields_null_when_absent() {
        let body = "Rs 1,500.00 spent on your HDFC Bank credit card at Amazon on 25-May-23.";
        let fields = detect_fx_fields(body, "INR");
        assert_eq!(fields.original_currency, None);
        assert_eq!(fields.original_amount_minor, None);
        assert_eq!(fields.exchange_rate, None);
    }

    #[test]
    fn test_settled_inr_amount_always_populated() {
        let body = "INR 1,500.00 debited from your account.";
        let fields = detect_fx_fields(body, "INR");
        assert_eq!(
            fields.original_currency, None,
            "the settled currency itself must never be treated as a foreign currency"
        );

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
