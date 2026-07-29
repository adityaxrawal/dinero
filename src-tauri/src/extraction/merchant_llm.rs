//! Issue #12: the LLM half of the user-triggered "Normalize with LLM" pass.
//!
//! Prompt construction, grammar-constrained output schema, validation, and
//! -- the part that makes the *learning* half real -- synthesis of a working
//! merchant regex from the email the LLM just read.
//!
//! The model is asked for two different merchant strings, which is the whole
//! trick here:
//!
//! * `merchant_in_email` -- the exact substring of the body that names the
//!   counterparty. Verifiable against the source (so a hallucination is
//!   detectable) and, crucially, *locatable*, which is what lets
//!   [`crate::extraction::rule_synthesis::synthesize_span_regex`] anchor a real
//!   pattern around it.
//! * `merchant_name` -- the canonical display name, which frequently does
//!   **not** appear in the body at all ("RAZ*SWIGGY BANGALORE" -> "Swiggy").
//!
//! Asking for only the canonical name would make the answer unverifiable and
//! unanchorable; asking for only the verbatim span would leave the truncation
//! problem unsolved.

use serde::Deserialize;

/// What the model returns, before validation.
#[derive(Debug, Deserialize)]
pub struct MerchantLlmOutput {
    /// Verbatim span from the email body naming the counterparty.
    pub merchant_in_email: Option<String>,
    /// Canonical display name; need not appear in the body.
    pub merchant_name: Option<String>,
    /// Must be one of the category names supplied in the prompt.
    pub category: Option<String>,
    /// The model's own confidence, surfaced to the user and stored alongside
    /// the correction.
    pub confidence: Option<f64>,
}

/// A validated, usable answer.
#[derive(Debug, Clone, PartialEq)]
pub struct MerchantResolution {
    pub merchant_in_email: String,
    pub merchant_name: String,
    pub category: String,
    pub confidence: f64,
}

/// Grammar-constrained output shape. `category` is an `enum` of the caller's
/// closed list, so the sampler physically cannot emit a category that isn't
/// in the database -- this is why no post-hoc "did it invent a category"
/// repair path is needed.
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

/// The transaction context sent alongside the body, so the model can tell a
/// refund from a purchase and a counterparty from the user's own bank.
pub struct TransactionContext<'a> {
    pub bank_name: &'a str,
    pub current_merchant: &'a str,
    pub amount: Option<f64>,
    pub currency: Option<&'a str>,
    pub direction: Option<&'a str>,
    pub event_time: Option<&'a str>,
}

/// Bodies longer than this are truncated before being sent. Bank alert emails
/// carry the transaction facts near the top and legal boilerplate at the
/// bottom, so a head-truncation loses almost nothing while keeping the prompt
/// inside the sidecar's context window.
const MAX_BODY_CHARS: usize = 4000;

fn truncate_body(body: &str) -> String {
    if body.chars().count() <= MAX_BODY_CHARS {
        return body.to_string();
    }
    let head: String = body.chars().take(MAX_BODY_CHARS).collect();
    format!("{head}\n[...truncated...]")
}

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
        body = truncate_body(body),
        category_list = category_list,
    )
}

/// Validates one raw completion against the source body and the closed
/// category list.
///
/// Returns `None` for every failure mode -- unparseable, no merchant found,
/// a `merchant_in_email` that does not actually occur in the body (the
/// hallucination guard), or a category outside the list. A rejected answer
/// leaves the transaction untouched rather than writing a guess.
pub fn validate(
    raw_output: &str,
    body: &str,
    categories: &[String],
) -> Option<MerchantResolution> {
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

    // Hallucination guard: the span the model claims to have copied must
    // really be there. This is what makes it safe to synthesize an
    // immediately-active pattern rule from the answer.
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

    /// The guard that makes immediate rule activation safe.
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
    fn long_bodies_are_truncated() {
        let body = "x".repeat(MAX_BODY_CHARS * 2);
        assert!(truncate_body(&body).chars().count() < MAX_BODY_CHARS + 40);
    }
}
