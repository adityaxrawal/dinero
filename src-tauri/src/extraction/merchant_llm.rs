//! LLM-assisted merchant identification and categorisation.
//!
//! Used where deterministic normalisation cannot resolve a descriptor. Prompt
//! construction, the response schema, and validation live together here so the
//! contract with the model stays consistent, and validation rejects output the
//! schema alone would let through.
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct MerchantLlmOutput {
    pub merchant_in_email: Option<String>,
    pub merchant_name: Option<String>,
    pub category: Option<String>,
    pub confidence: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MerchantResolution {
    pub merchant_in_email: String,
    pub merchant_name: String,
    pub category: String,
    pub confidence: f64,
}

/// Schema for merchant normalisation, restricted to existing categories.
///
/// Constraining the category set at the schema level stops the model inventing
/// new categories, which would fragment the user's spending breakdown.
pub fn merchant_cleanup_schema(categories: &[String]) -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "merchant_in_email": {"type": ["string", "null"]},
            "merchant_name": {"type": ["string", "null"]},
            "category": {"enum": categories},
            "confidence": {"type": "number"}
        },
        "required": ["merchant_in_email", "merchant_name", "category", "confidence"]
    })
}

pub struct TransactionContext<'a> {
    pub bank_name: &'a str,
    pub current_merchant: &'a str,
    pub amount: Option<f64>,
    pub currency: Option<&'a str>,
    pub direction: Option<&'a str>,
    pub event_time: Option<&'a str>,
}

/// Builds the merchant-resolution prompt from a transaction and its source text.
pub fn generate_prompt(ctx: &TransactionContext, body: &str, categories: &[String]) -> String {
    let amount = ctx
        .amount
        .map(|a| format!("{a:.2}"))
        .unwrap_or_else(|| "unknown".to_string());
    let category_list = categories.join(", ");

    format!(
        "You are correcting the merchant name on a bank transaction that an automated \
         parser extracted badly.\n\n\
         The parser read this email and pulled out the merchant \"{current}\", which is \
         wrong, incomplete, or truncated. Read the email yourself and answer with the \
         real counterparty.\n\n\
         Return ONLY valid JSON with these four fields:\n\
         - merchant_in_email: the exact text from the email that names the merchant, \
           copied verbatim, character for character. It MUST appear in the email body \
           below. Copy it exactly as written, including any prefix like \"RAZ*\" or \
           suffix like \"*BANGALORE\".\n\
         - merchant_name: the real-world brand name, cleaned up and spelled in full. \
           This does NOT need to appear in the email. Expand truncations and drop \
           gateway prefixes, city suffixes and terminal codes.\n\
         - category: exactly one of: {category_list}\n\
         - confidence: a number from 0.0 to 1.0, how sure you are. Use a LOW value \
           (below 0.5) if the email does not clearly name a merchant at all.\n\n\
         Rules:\n\
         - The merchant is the OTHER party, never the user's own bank ({bank}) and never \
           the user themselves.\n\
         - A payment gateway (Razorpay, PayU, PhonePe, Paytm, BillDesk, CCAvenue) is not \
           the merchant. If the text is \"RAZ*SWIGGY\", the merchant is Swiggy.\n\
         - If the email genuinely names no merchant (a balance summary, an OTP, a \
           statement notice), set merchant_in_email and merchant_name to null and \
           confidence to 0.0.\n\n\
         Example:\n\
         Email: \"Rs.245.43 spent on your SBI Credit Card ending 7603 at RAZ*SWIGGY \
         LIMITE BANGALORE on 01/07/26.\"\n\
         Output: {{\"merchant_in_email\": \"RAZ*SWIGGY LIMITE BANGALORE\", \
         \"merchant_name\": \"Swiggy\", \"category\": \"Food & Dining\", \"confidence\": 0.95}}\n\n\
         Now the real one.\n\
         Bank: {bank}\n\
         Amount: {amount} {currency}\n\
         Direction: {direction}\n\
         Date: {date}\n\
         Parser's current (wrong) merchant: \"{current}\"\n\
         Email Body:\n\
         \"\"\"\n{body}\n\"\"\"\n\
         JSON Output:",
        current = ctx.current_merchant,
        bank = ctx.bank_name,
        amount = amount,
        currency = ctx.currency.unwrap_or("INR"),
        direction = ctx.direction.unwrap_or("unknown"),
        date = ctx.event_time.unwrap_or("unknown"),
        body = body,
        category_list = category_list,
    )
}

/// Validates a model response against the message it came from.
///
/// Checks the proposed name is grounded in the actual body rather than invented,
/// and that the category is one that exists. Returning None rejects the
/// suggestion outright -- keeping the raw descriptor is better than adopting a
/// plausible-sounding fabrication.
pub fn validate(raw_output: &str, body: &str, categories: &[String]) -> Option<MerchantResolution> {
    let json_text = crate::extraction::llm::LlmEngine::extract_json_block(raw_output)?;
    let parsed: MerchantLlmOutput = serde_json::from_str(json_text).ok()?;

    let in_email = parsed.merchant_in_email?;
    let name = parsed.merchant_name?;
    let category = parsed.category?;
    let in_email = in_email.trim().to_string();
    let name = name.trim().to_string();
    if in_email.is_empty() || name.is_empty() {
        return None;
    }

    if crate::extraction::rule_synthesis::find_ignore_case(body, &in_email).is_none() {
        tracing::debug!(
            merchant_in_email = %in_email,
            "merchant cleanup: rejected, claimed span absent from source body"
        );
        return None;
    }

    if !categories.iter().any(|c| c == &category) {
        tracing::debug!(%category, "merchant cleanup: rejected, category outside closed list");
        return None;
    }

    Some(MerchantResolution {
        merchant_in_email: in_email,
        merchant_name: name,
        category,
        confidence: parsed.confidence.unwrap_or(0.0).clamp(0.0, 1.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SBI_BODY: &str = "Dear Cardholder, Rs.245.43 spent on your SBI Credit Card \
                            ending 7603 at RAZ*SWIGGY LIMITE BANGALORE on 01/07/26. \
                            Not you? Call 18001234.";

    fn cats() -> Vec<String> {
        vec![
            "Food & Dining".to_string(),
            "Shopping".to_string(),
            "Others".to_string(),
        ]
    }

    #[test]
    fn accepts_a_well_formed_answer() {
        let raw = r#"{"merchant_in_email": "RAZ*SWIGGY LIMITE BANGALORE",
                      "merchant_name": "Swiggy", "category": "Food & Dining",
                      "confidence": 0.95}"#;
        let r = validate(raw, SBI_BODY, &cats()).expect("must accept");
        assert_eq!(r.merchant_name, "Swiggy");
        assert_eq!(r.category, "Food & Dining");
        assert_eq!(r.confidence, 0.95);
    }

    #[test]
    fn rejects_a_span_that_is_not_in_the_body() {
        let raw = r#"{"merchant_in_email": "ZOMATO", "merchant_name": "Zomato",
                      "category": "Food & Dining", "confidence": 0.9}"#;
        assert!(
            validate(raw, SBI_BODY, &cats()).is_none(),
            "a merchant the email never mentions must be rejected"
        );
    }

    #[test]
    fn rejects_a_category_outside_the_closed_list() {
        let raw = r#"{"merchant_in_email": "RAZ*SWIGGY LIMITE BANGALORE",
                      "merchant_name": "Swiggy", "category": "Invented Category",
                      "confidence": 0.9}"#;
        assert!(validate(raw, SBI_BODY, &cats()).is_none());
    }

    #[test]
    fn rejects_the_no_merchant_answer() {
        let raw = r#"{"merchant_in_email": null, "merchant_name": null,
                      "category": "Others", "confidence": 0.0}"#;
        assert!(validate(raw, SBI_BODY, &cats()).is_none());
    }

    #[test]
    fn schema_pins_category_to_the_closed_list() {
        let schema = merchant_cleanup_schema(&cats());
        assert_eq!(
            schema["properties"]["category"]["enum"],
            serde_json::json!(["Food & Dining", "Shopping", "Others"]),
            "the grammar must make an invented category unrepresentable"
        );
    }

    #[test]
    fn long_bodies_are_not_truncated() {
        let body = "x".repeat(10_000);
        let ctx = TransactionContext {
            bank_name: "SBI",
            current_merchant: "RAZ*",
            amount: Some(100.0),
            currency: None,
            direction: None,
            event_time: None,
        };
        let prompt = generate_prompt(&ctx, &body, &cats());
        assert!(prompt.contains(&body));
        assert!(!prompt.contains("[...truncated...]"));
    }
}
