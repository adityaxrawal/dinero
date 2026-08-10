//! Extracts standing-instruction and auto-debit mandate details.
//!
//! Mandates announce future recurring charges. Capturing them lets an upcoming
//! debit be anticipated rather than only recognised after the money has left.
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

use crate::extraction::normalization::clean_masked_identifier;

static MERCHANT_RE: OnceLock<Regex> = OnceLock::new();
static CADENCE_RE: OnceLock<Regex> = OnceLock::new();
static AMOUNT_RE: OnceLock<Regex> = OnceLock::new();
static MANDATE_ID_RE: OnceLock<Regex> = OnceLock::new();
static CARD_LAST4_RE: OnceLock<Regex> = OnceLock::new();

/// Extracts standing-instruction details from a mandate notification.
///
/// Mandates announce charges that have not happened yet, which is what allows a
/// future debit to be anticipated rather than merely recognised afterwards.
pub fn extract_mandate_fields(bank_name: &str, body: &str) -> Option<MandateExtraction> {
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

    let card_last4_re = CARD_LAST4_RE
        .get_or_init(|| Regex::new(r"(?i)ending\s*(?:in\s+)?(?:[Xx*\s\-.]*?)(\d{2,4})\b").unwrap());
    let masked_identifier = card_last4_re
        .captures(body)
        .and_then(|c| c.get(1))
        .map(|m| clean_masked_identifier(m.as_str()));

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

/// Applies a bank-specific mandate template, where one exists.
pub fn bank_mandate_template(bank_name: &str, body: &str) -> Option<MandateExtraction> {
    let patterns = crate::extraction::ladder::bank_templates().get(bank_name)?;

    for p in patterns {
        if p.txn_type.as_deref() != Some("mandate") {
            continue;
        }
        let Some(caps) = p.regex.captures(body) else {
            continue;
        };
        let group = |g: Option<usize>| {
            g.and_then(|g| caps.get(g))
                .map(|m| m.as_str().trim().to_string())
                .filter(|s| !s.is_empty())
        };

        let Some(merchant) = group(p.merchant_group) else {
            continue;
        };

        return Some(MandateExtraction {
            merchant: Some(merchant),
            cadence: group(p.cadence_group).map(|c| c.to_lowercase()),
            max_limit_amount: caps
                .get(p.amount_group)
                .and_then(|m| m.as_str().replace(',', "").parse::<f64>().ok())
                .map(|f| (f * 100.0).round() as i64),
            external_mandate_id: group(p.reference_group),
            instrument_type: Some(
                if body.to_lowercase().contains("credit card") {
                    "credit_card"
                } else {
                    "bank_account"
                }
                .to_string(),
            ),
            issuer_name: Some(bank_name.to_string()),
            masked_identifier: group(p.last4_group).map(|d| clean_masked_identifier(&d)),
        });
    }

    None
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
        let body = "Thank you for registering for a recurring e-Mandate at merchant platform using your SBI Credit Card. Merchant: ScribdInc";
        let result = extract_mandate_fields("SBI Card", body).unwrap();
        assert_eq!(result.merchant, Some("ScribdInc".to_string()));
    }
}
