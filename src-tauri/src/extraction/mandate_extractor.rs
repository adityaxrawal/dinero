use regex::Regex;
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct MandateExtraction {
    pub merchant: Option<String>,
    pub cadence: Option<String>,
    pub max_limit_amount: Option<i64>,
    pub external_mandate_id: Option<String>,
    pub instrument_type: Option<String>,
    pub issuer_name: Option<String>,
    pub masked_identifier: Option<String>,
}

static MERCHANT_RE: OnceLock<Regex> = OnceLock::new();
static CADENCE_RE: OnceLock<Regex> = OnceLock::new();
static AMOUNT_RE: OnceLock<Regex> = OnceLock::new();
static MANDATE_ID_RE: OnceLock<Regex> = OnceLock::new();
static CARD_LAST4_RE: OnceLock<Regex> = OnceLock::new();

/// Extracts mandate-lifecycle fields from a mandate registration/cancellation
/// email body. `merchant` is the only mandatory field (mirrors Gate 3's
/// precision-over-recall discipline, Doc 12 §6.2) -- returns `None` entirely
/// if it can't be found, same "reject rather than guess" posture as every
/// other gate in this pipeline
/// (docs/superpowers/specs/2026-07-18-mandate-tracking-design.md §4.3).
pub fn extract_mandate_fields(bank_name: &str, body: &str) -> Option<MandateExtraction> {
    // Reuses GenericRegexLayer's merchant-keyword convention
    // (ladder.rs GENERIC_MERCHANT_RE_STRICT), but with the colon *required*
    // (`:` not `:?`) -- found via a real-body test failure: mandate emails'
    // boilerplate text uses "merchant" as an ordinary English word before
    // the real label ("...registering for a recurring e-Mandate at
    // merchant platform using your..."), which the optional-colon version
    // matched first (leftmost), well before the actual "Merchant:
    // ScribdInc" label later in the body. The real label is always
    // colon-terminated in every template seen; the boilerplate usage never
    // is, so requiring the colon disambiguates them.
    let merchant_re = MERCHANT_RE.get_or_init(|| {
        Regex::new(r"(?i)\b(?:merchant name|merchant):\s+([A-Za-z0-9\s*]{2,40}?)(?:\s+description\b|\s+on\b|[,.\n\-]|$)").unwrap()
    });
    let merchant = merchant_re
        .captures(body)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
        .filter(|m| !m.is_empty())?;

    let cadence_re = CADENCE_RE.get_or_init(|| {
        Regex::new(r"(?i)frequency:?\s+(monthly|weekly|daily|yearly|quarterly)").unwrap()
    });
    let cadence = cadence_re
        .captures(body)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_lowercase());

    let amount_re = AMOUNT_RE.get_or_init(|| {
        Regex::new(
            r"(?i)(?:limit amount|max limit)\s*(?:\(inr\))?:?\s*(?:inr)?\s*([\d,]+(?:\.\d{1,2})?)",
        )
        .unwrap()
    });
    let max_limit_amount = amount_re
        .captures(body)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().replace(',', "").parse::<f64>().ok())
        .map(|f| (f * 100.0).round() as i64);

    let mandate_id_re = MANDATE_ID_RE.get_or_init(|| {
        Regex::new(r"(?i)(?:sihub id|mandate id|mandate reference|umrn):?\s+([A-Za-z0-9]{4,20})")
            .unwrap()
    });
    let external_mandate_id = mandate_id_re
        .captures(body)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string());

    let card_last4_re = CARD_LAST4_RE.get_or_init(|| Regex::new(r"(?i)ending\s+(\d{4})").unwrap());
    let masked_identifier = card_last4_re
        .captures(body)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string());

    let instrument_type = if body.to_lowercase().contains("credit card") {
        Some("credit_card".to_string())
    } else {
        None
    };

    Some(MandateExtraction {
        merchant: Some(merchant),
        cadence,
        max_limit_amount,
        external_mandate_id,
        instrument_type,
        issuer_name: Some(bank_name.to_string()),
        masked_identifier,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_mandate_fields_sbi_card_registration() {
        let body = "Dear Cardholder, Thank you for registering for a recurring e-Mandate at merchant platform using your SBI Credit Card. Your e-Mandate set at merchant with SBI Credit Card ending 7603 has been registered. Merchant: ScribdInc Description: PremiumMonthlyMembership e-Mandate Limit Amount (INR): 1000.00 Frequency: monthly Start date: 21/04/2026 End date: 21/04/2046 SiHub ID: YPCojLhIn2 Also, please note that you have authorised debit of INR. 0.00 from your account towards the first Trxn. against this e-Mandate.";
        let result = extract_mandate_fields("SBI Card", body).unwrap();
        assert_eq!(result.merchant, Some("ScribdInc".to_string()));
        assert_eq!(result.cadence, Some("monthly".to_string()));
        assert_eq!(result.max_limit_amount, Some(100000));
        assert_eq!(result.external_mandate_id, Some("YPCojLhIn2".to_string()));
        assert_eq!(result.instrument_type, Some("credit_card".to_string()));
        assert_eq!(result.masked_identifier, Some("7603".to_string()));
    }

    #[test]
    fn test_extract_mandate_fields_sbi_card_cancellation() {
        let body = "Dear Cardholder, Thank you for registering for a recurring E-mandate at merchant platform using your SBI Credit Card. We observe that you have cancelled your E-mandate for SiHub ID: YPCojLhIn2 on SBI Credit Card ending 7603. The below E-mandate stands cancelled: Merchant: ScribdInc Description: PremiumMonthlyMembership";
        let result = extract_mandate_fields("SBI Card", body).unwrap();
        assert_eq!(result.merchant, Some("ScribdInc".to_string()));
        assert_eq!(result.external_mandate_id, Some("YPCojLhIn2".to_string()));
        assert_eq!(result.masked_identifier, Some("7603".to_string()));
    }

    #[test]
    fn test_extract_mandate_fields_returns_none_without_merchant() {
        let body = "Your recurring payment mandate has been updated. No counterparty label is present anywhere in this text.";
        assert!(extract_mandate_fields("Any Bank", body).is_none());
    }

    #[test]
    fn test_extract_mandate_fields_bare_merchant_word_in_boilerplate_not_matched() {
        // Regression test for the leftmost-match bug: "at merchant platform"
        // (ordinary English, no colon) must not be captured as the
        // merchant, even though it appears before the real "Merchant:
        // ScribdInc" label.
        let body = "Thank you for registering for a recurring e-Mandate at merchant platform using your SBI Credit Card. Merchant: ScribdInc";
        let result = extract_mandate_fields("SBI Card", body).unwrap();
        assert_eq!(result.merchant, Some("ScribdInc".to_string()));
    }
}
