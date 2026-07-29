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
//!   [`synthesize_merchant_regex`] anchor a real pattern around it.
//! * `merchant_name` -- the canonical display name, which frequently does
//!   **not** appear in the body at all ("RAZ*SWIGGY BANGALORE" -> "Swiggy").
//!
//! Asking for only the canonical name would make the answer unverifiable and
//! unanchorable; asking for only the verbatim span would leave the truncation
//! problem unsolved.

use serde::Deserialize;

/// Longest merchant span the synthesized regex will capture. Generous enough
/// for "UTTAR PRADESH STATE ROAD TRANSPORT CORPORATION", short enough that a
/// mis-anchored pattern can't swallow a whole paragraph.
const MAX_MERCHANT_CAPTURE: usize = 80;

/// How much literal text on each side of the merchant is baked into the
/// anchor. Long enough to be specific to this email's phrasing, short enough
/// to survive the small wording differences between two alerts of the same
/// kind.
const ANCHOR_CHARS: usize = 24;

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

/// Case-insensitive search for `needle` in `haystack`, returning a byte range
/// into `haystack`. Used both to validate the model's verbatim claim and to
/// locate the anchor context.
fn find_ignore_case(haystack: &str, needle: &str) -> Option<std::ops::Range<usize>> {
    if needle.is_empty() {
        return None;
    }
    let hay_lower = haystack.to_lowercase();
    let needle_lower = needle.to_lowercase();
    // `to_lowercase` can change byte length (e.g. 'İ'), which would make an
    // index into the lowered string invalid for the original. Bail out rather
    // than slice at a wrong offset; these bodies are ASCII in practice.
    if hay_lower.len() != haystack.len() || needle_lower.len() != needle.len() {
        return haystack.find(needle).map(|s| s..s + needle.len());
    }
    hay_lower
        .find(&needle_lower)
        .map(|s| s..s + needle_lower.len())
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
    if find_ignore_case(body, &in_email).is_none() {
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

/// Turns a literal chunk of email text into a regex fragment that still
/// matches the *next* email of the same kind.
///
/// Two relaxations, both load-bearing:
/// * whitespace runs become `\s+`, because HTML flattening produces wildly
///   different blank-line counts between two renderings of the same template;
/// * digit runs become `\d+`, because the anchor otherwise bakes in this
///   transaction's card digits or amount and never matches again. This mirrors
///   `compute_template_hash`, which also collapses digits -- so a rule keyed
///   to a template hash stays consistent with the anchor built for it.
fn relax_literal(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_whitespace() {
            while chars.peek().is_some_and(|n| n.is_whitespace()) {
                chars.next();
            }
            out.push_str(r"\s+");
        } else if c.is_ascii_digit() {
            while chars.peek().is_some_and(|n| n.is_ascii_digit()) {
                chars.next();
            }
            out.push_str(r"\d+");
        } else {
            out.push_str(&regex::escape(&c.to_string()));
        }
    }
    out
}

/// Builds a merchant-extraction regex anchored on the literal text
/// surrounding `merchant_in_email` inside `body`, with capture group 1 on the
/// merchant itself (the group `LearnedPatternLayer` reads).
///
/// Returns `None` unless the finished pattern compiles *and* re-extracts the
/// expected merchant from the very body it was built from. That self-check is
/// what makes an immediately-active rule safe: a pattern that cannot even
/// reproduce its own training example is never stored.
pub fn synthesize_merchant_regex(body: &str, merchant_in_email: &str) -> Option<String> {
    let span = find_ignore_case(body, merchant_in_email)?;

    // Take context on each side, snapped to a char boundary.
    let prefix_start = body[..span.start]
        .char_indices()
        .rev()
        .take(ANCHOR_CHARS)
        .last()
        .map(|(i, _)| i)
        .unwrap_or(span.start);
    let suffix_end = body[span.end..]
        .char_indices()
        .take(ANCHOR_CHARS)
        .last()
        .map(|(i, c)| span.end + i + c.len_utf8())
        .unwrap_or(span.end);

    let prefix = relax_literal(&body[prefix_start..span.start]);
    let suffix = relax_literal(&body[span.end..suffix_end]);

    // No anchor on either side would make the pattern match anything at all.
    if prefix.is_empty() && suffix.is_empty() {
        return None;
    }

    let pattern = format!("(?is){prefix}(.{{1,{MAX_MERCHANT_CAPTURE}}}?){suffix}");

    // Self-check: compile, and confirm it recovers the merchant it was built
    // from. Guards against an anchor whose own relaxation (e.g. a `\d+` that
    // now also matches part of the merchant) shifts the capture.
    let re = regex::Regex::new(&pattern).ok()?;
    let recovered = re.captures(body)?.get(1)?.as_str().trim();
    if !recovered.eq_ignore_ascii_case(merchant_in_email.trim()) {
        tracing::debug!(
            expected = %merchant_in_email,
            recovered = %recovered,
            "merchant cleanup: synthesized regex failed its own self-check, discarding"
        );
        return None;
    }

    Some(pattern)
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

    /// The synthesized rule must survive the thing that broke every previous
    /// attempt at this: the *next* email has different digits and different
    /// whitespace, and must still match.
    #[test]
    fn synthesized_regex_generalises_to_the_next_email() {
        let pattern = synthesize_merchant_regex(SBI_BODY, "RAZ*SWIGGY LIMITE BANGALORE")
            .expect("must synthesize");
        let re = regex::Regex::new(&pattern).unwrap();

        // Same template, different card digits, amount, date, and merchant.
        let next = "Dear Cardholder, Rs.1,020.00 spent on your SBI Credit Card \
                    ending 4412 at RAZ*YULU BIKES on 14/07/26. Not you? Call 18009999.";
        let caps = re
            .captures(next)
            .expect("the learned rule must fire on the next email of this shape");
        assert_eq!(caps.get(1).unwrap().as_str().trim(), "RAZ*YULU BIKES");
    }

    /// Whitespace relaxation: the HTML-flattened rendering of the same email
    /// has different blank-line counts between fields.
    #[test]
    fn synthesized_regex_survives_reflowed_whitespace() {
        let pattern = synthesize_merchant_regex(SBI_BODY, "RAZ*SWIGGY LIMITE BANGALORE").unwrap();
        let re = regex::Regex::new(&pattern).unwrap();
        let reflowed = SBI_BODY.replace(' ', "\n\n  ");
        assert!(
            re.captures(&reflowed).is_some(),
            "pattern must tolerate reflowed whitespace"
        );
    }

    #[test]
    fn synthesis_refuses_a_merchant_absent_from_the_body() {
        assert!(synthesize_merchant_regex(SBI_BODY, "ZOMATO").is_none());
    }

    /// Capture group 1 is what `LearnedPatternLayer` reads
    /// (`ladder.rs:163`), so the group index is part of the contract.
    #[test]
    fn merchant_lands_in_capture_group_one() {
        let pattern = synthesize_merchant_regex(SBI_BODY, "RAZ*SWIGGY LIMITE BANGALORE").unwrap();
        let re = regex::Regex::new(&pattern).unwrap();
        assert_eq!(
            re.captures(SBI_BODY).unwrap().get(1).unwrap().as_str().trim(),
            "RAZ*SWIGGY LIMITE BANGALORE"
        );
    }

    #[test]
    fn relax_literal_collapses_digits_and_whitespace() {
        assert_eq!(relax_literal("ending 7603 at"), r"ending\s+\d+\s+at");
    }

    /// `*` and `.` are regex metacharacters and appear constantly in real
    /// merchant descriptors; an unescaped anchor would silently match the
    /// wrong thing.
    #[test]
    fn relax_literal_escapes_metacharacters() {
        let out = relax_literal("a*b.c");
        assert!(regex::Regex::new(&out).unwrap().is_match("a*b.c"));
        assert!(!regex::Regex::new(&out).unwrap().is_match("axbxc"));
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
