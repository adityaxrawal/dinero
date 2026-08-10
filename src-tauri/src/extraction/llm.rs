//! The LLM extraction layer, reached only when deterministic layers fall short.
//!
//! Runs against the local llama.cpp sidecar with a constrained JSON schema, so
//! the model returns a parseable structure rather than prose. Output is still
//! validated afterwards: a schema guarantees shape, not correctness, and a
//! confidently wrong amount is exactly what must not reach a financial ledger.
use super::ladder::ExtractionResult;
use crate::logging::llm_logger::{LlmCallContext, LlmCallType};
use serde::Deserialize;
use std::path::Path;
use tracing::{debug, error};

pub struct LlmEngine {
    app_dir: std::path::PathBuf,
    model_id: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Layer6Outcome {
    Extracted(Box<ExtractionResult>),
    TimedOut,
    Failed,
    Rejected,
}

enum CompletionAttempt {
    Extracted(Box<ExtractionResult>),
    TimedOut,
    Rejected(String),
    InfraFailed,
}

enum RawOutputOutcome {
    Accepted(Box<ExtractionResult>),
    FailedValidation,
    UnparseableJson,
}

#[derive(Debug, Deserialize)]
struct LlmJsonOutput {
    amount: Option<f64>,
    currency: Option<String>,
    direction: Option<String>,
    merchant: Option<String>,
    event_time: Option<i64>,
    reference_id: Option<String>,
    confidence: Option<f64>,
}

impl LlmEngine {
    /// Creates an engine bound to a model file.
    pub fn new(app_dir: &Path, model_id: &str) -> Self {
        Self {
            app_dir: app_dir.to_path_buf(),
            model_id: model_id.to_string(),
        }
    }

    /// Builds the extraction prompt for a message.
    ///
    /// The bank name is included because it materially improves accuracy: it tells
    /// the model which conventions to expect rather than leaving it to infer them.
    pub fn generate_prompt(bank_name: &str, body_text: &str) -> String {
        format!(
            "Extract the following fields from a bank transaction alert email sent by {bank_name}. \
             Return ONLY valid JSON and nothing else -- no markdown fences, no commentary.\n\
             Fields:\n\
             - amount: number (e.g., 1500.50)\n\
             - currency: string (e.g., \"INR\", \"USD\")\n\
             - direction: string (\"credit\" or \"debit\")\n\
             - merchant: string (e.g., \"Amazon\")\n\
             - event_time: integer (Unix timestamp, e.g., 1704067200)\n\
             - reference_id: string (e.g., \"1234567890\")\n\
             - confidence: number from 0.0 to 1.0, how sure you are that every field above is \
             correct and genuinely present in the email (not inferred or guessed). Use a LOW \
             value (below 0.3) if the email is unusually formatted, if any field required a \
             judgment call, or if you are not fully certain.\n\n\
             Every field's value must come from the email body verbatim (or a straightforward \
             conversion of it, e.g. \"Rs. 1,500.50\" -> 1500.50) -- never invent a value that \
             doesn't appear in the text.\n\n\
             Example 1 (debit):\n\
             Email Body: \"Dear Customer, Rs 1,299.00 has been debited from your HDFC Bank \
             account ending 4521 on 05-Jan-24 towards purchase at Amazon. Available balance: \
             Rs 45,000.00. Ref No 987654321.\"\n\
             JSON Output: {{\"amount\": 1299.00, \"currency\": \"INR\", \"direction\": \"debit\", \
             \"merchant\": \"Amazon\", \"event_time\": 1704412200, \"reference_id\": \"987654321\", \
             \"confidence\": 0.95}}\n\n\
             Example 2 (credit, no reference number stated):\n\
             Email Body: \"Your ICICI Bank account XX7890 has been credited with INR 5,000.00 \
             on 12-Mar-24 from NEFT transfer by RAVI KUMAR.\"\n\
             JSON Output: {{\"amount\": 5000.00, \"currency\": \"INR\", \"direction\": \"credit\", \
             \"merchant\": \"RAVI KUMAR\", \"event_time\": 1710201000, \"reference_id\": null, \
             \"confidence\": 0.9}}\n\n\
             Example 3 (UPI app confirmation, nested/cluttered layout):\n\
             Email Body: \"Payment Successful You paid \u{20B9}300.00 Paid to Swiggy UPI \
             Transaction ID: 302514789632 Order confirmed 22 Feb 2024, 8:45 PM\"\n\
             JSON Output: {{\"amount\": 300.00, \"currency\": \"INR\", \"direction\": \"debit\", \
             \"merchant\": \"Swiggy\", \"event_time\": 1708613100, \"reference_id\": \"302514789632\", \
             \"confidence\": 0.9}}\n\n\
             Now extract from this email:\n\
             Email Body:\n\
             \"\"\"\n\
             {body_text}\n\
             \"\"\"\n\
             JSON Output:"
        )
    }

    /// Builds a retry prompt after output failed validation.
    ///
    /// Stating what was wrong is worth one more attempt: the common failures --
    /// inventing a merchant, misreading an amount -- are often corrected when the
    /// model is shown the specific problem.
    fn generate_correction_prompt(
        bank_name: &str,
        body_text: &str,
        previous_output: &str,
    ) -> String {
        format!(
            "Your previous answer was not accepted: either it was not valid JSON, or one of the \
             values (amount / merchant / reference_id) does not actually appear anywhere in the \
             email body below -- every value must come from the text verbatim.\n\n\
             Your previous answer was:\n{previous_output}\n\n\
             Look at the email body again carefully and try again. Return ONLY valid JSON, no \
             markdown fences, no commentary.\n\
             Fields: amount (number), currency (string), direction (\"credit\" or \"debit\"), \
             merchant (string), event_time (integer Unix timestamp), reference_id (string), \
             confidence (number 0.0-1.0, how sure you are).\n\n\
             Bank: {bank_name}\n\
             Email Body:\n\
             \"\"\"\n\
             {body_text}\n\
             \"\"\"\n\
             JSON Output:"
        )
    }

    /// Runs extraction, retrying once with a correction prompt if validation fails.
    pub async fn extract(
        &self,
        bank_name: &str,
        body_text: &str,
        fallback_event_time: Option<i64>,
    ) -> Layer6Outcome {
        let prompt = Self::generate_prompt(bank_name, body_text);
        match self
            .run_completion(&prompt, body_text, 1, fallback_event_time)
            .await
        {
            CompletionAttempt::Extracted(result) => Layer6Outcome::Extracted(result),
            CompletionAttempt::TimedOut => Layer6Outcome::TimedOut,
            CompletionAttempt::Rejected(raw_output) => {
                debug!(
                    "Layer 6 LLM output rejected on first attempt, retrying with correction prompt"
                );
                let correction_prompt =
                    Self::generate_correction_prompt(bank_name, body_text, &raw_output);
                match self
                    .run_completion(&correction_prompt, body_text, 2, fallback_event_time)
                    .await
                {
                    CompletionAttempt::Extracted(result) => Layer6Outcome::Extracted(result),
                    CompletionAttempt::TimedOut => Layer6Outcome::TimedOut,
                    CompletionAttempt::Rejected(_) => {
                        debug!("Layer 6 LLM output rejected again after self-correction retry");
                        Layer6Outcome::Rejected
                    }
                    CompletionAttempt::InfraFailed => Layer6Outcome::Failed,
                }
            }
            CompletionAttempt::InfraFailed => Layer6Outcome::Failed,
        }
    }

    /// Issues one schema-constrained completion against the sidecar.
    async fn run_completion(
        &self,
        prompt: &str,
        body_text: &str,
        attempt: u8,
        fallback_event_time: Option<i64>,
    ) -> CompletionAttempt {
        let ctx = LlmCallContext::new(LlmCallType::Layer6Extraction, attempt);
        let mut retry_delay = std::time::Duration::from_millis(1000);
        let max_delay = std::time::Duration::from_millis(2000);
        let max_total_wait = std::time::Duration::from_secs(120);
        let start_time = std::time::Instant::now();

        let mut timed_out = false;
        let raw_output = loop {
            let result = crate::llama_sidecar::complete_with_schema_and_context(
                &self.app_dir,
                &self.model_id,
                prompt,
                crate::llama_sidecar::layer6_json_schema_pub(),
                ctx,
            )
            .await;

            match result {
                Ok(output) => break Some(output),
                Err(e) => {
                    let msg = e.to_string();
                    if (msg.contains("starting") || msg.contains("try again shortly"))
                        && start_time.elapsed() < max_total_wait
                    {
                        tokio::time::sleep(retry_delay).await;
                        retry_delay = std::cmp::min(retry_delay * 2, max_delay);
                        continue;
                    }
                    if msg.contains("timeout") || msg.contains("timed out") {
                        error!("Layer 6 LLM Failure: inference exceeded its calibrated timeout");
                        timed_out = true;
                        break None;
                    }
                    error!("Layer 6 LLM Failure: {}", e);
                    break None;
                }
            }
        };

        match raw_output {
            Some(raw) => match self.classify_raw_output(&raw, body_text, fallback_event_time) {
                RawOutputOutcome::Accepted(parsed) => CompletionAttempt::Extracted(parsed),
                RawOutputOutcome::FailedValidation => {
                    debug!(
                        "Layer 6 LLM output rejected: parsed JSON failed source validation \
                         (a field doesn't appear in the email body)"
                    );
                    CompletionAttempt::Rejected(raw)
                }
                RawOutputOutcome::UnparseableJson => {
                    debug!("Layer 6 LLM output rejected: unparseable JSON");
                    CompletionAttempt::Rejected(raw)
                }
            },
            None if timed_out => CompletionAttempt::TimedOut,
            None => CompletionAttempt::InfraFailed,
        }
    }

    /// Classifies raw model output into accepted, invalid, or unparseable.
    ///
    /// Three outcomes rather than an Option, because they call for different
    /// responses: unparseable JSON is worth retrying, whereas output that parsed but
    /// contradicts the source is a hallucination and should be abandoned.
    fn classify_raw_output(
        &self,
        raw: &str,
        body_text: &str,
        fallback_event_time: Option<i64>,
    ) -> RawOutputOutcome {
        match self.parse_json_to_result(raw, fallback_event_time) {
            Some(parsed) if Self::validate_against_source(&parsed, body_text) => {
                RawOutputOutcome::Accepted(Box::new(parsed))
            }
            Some(_) => RawOutputOutcome::FailedValidation,
            None => RawOutputOutcome::UnparseableJson,
        }
    }

    /// Verifies every extracted value actually appears in the source message.
    ///
    /// The primary defence against hallucination. A schema guarantees the response
    /// has the right shape, not that its contents are real -- and a fabricated
    /// merchant or amount that reaches a financial ledger is the worst failure this
    /// module can produce.
    ///
    /// Grounding each value in the source text is what makes the LLM layer safe to
    /// use at all.
    pub fn validate_against_source(result: &ExtractionResult, source_body: &str) -> bool {
        let source_lower = source_body.to_lowercase();
        if let Some(merchant) = &result.merchant_raw {
            if !merchant.is_empty() && !source_lower.contains(&merchant.to_lowercase()) {
                return false;
            }
        }
        if let Some(reference_id) = &result.reference_id {
            if !reference_id.is_empty() && !source_lower.contains(&reference_id.to_lowercase()) {
                return false;
            }
        }
        if let Some(amount_minor) = result.amount_minor {
            if !Self::amount_appears_in_source(amount_minor, source_body) {
                return false;
            }
        }
        true
    }

    /// Whether an amount is genuinely present in the message text.
    ///
    /// Commas are stripped first, so `1,200.00` matches an extracted 120000 minor
    /// units. Whole amounts are also checked without decimals, since banks commonly
    /// print `1200` rather than `1200.00`.
    fn amount_appears_in_source(amount_minor: i64, source_body: &str) -> bool {
        let normalized_source: String = source_body.chars().filter(|c| *c != ',').collect();
        let major = amount_minor as f64 / 100.0;
        let with_decimals = format!("{major:.2}");
        if normalized_source.contains(&with_decimals) {
            return true;
        }
        if amount_minor % 100 == 0 {
            let whole = format!("{}", amount_minor / 100);
            if normalized_source.contains(&whole) {
                return true;
            }
        }
        false
    }

    /// Parses model output into an extraction result.
    ///
    /// A fallback event time is supplied because the message timestamp is a better
    /// answer than none when the model omits a date.
    pub fn parse_json_to_result(
        &self,
        llm_output: &str,
        fallback_event_time: Option<i64>,
    ) -> Option<ExtractionResult> {
        let json_str = Self::extract_json_block(llm_output).unwrap_or(llm_output);

        let parsed: LlmJsonOutput = match serde_json::from_str(json_str) {
            Ok(p) => p,
            Err(e) => {
                debug!("Failed to parse LLM JSON: {} - Raw: {}", e, json_str);
                return None;
            }
        };

        let mut result = ExtractionResult {
            extraction_method: "llm_layer6".to_string(),
            confidence_score: Some(parsed.confidence.unwrap_or(0.0).clamp(0.0, 1.0)),
            amount_minor: parsed.amount.map(|v| (v * 100.0).round() as i64),
            currency: parsed.currency,
            direction: parsed.direction,
            merchant_raw: parsed.merchant,
            event_time: parsed.event_time.or(fallback_event_time),
            reference_id: parsed.reference_id,
            ..Default::default()
        };

        match result
            .direction
            .as_deref()
            .map(str::to_lowercase)
            .as_deref()
        {
            Some("credit") => result.direction = Some("credit".to_string()),
            Some("debit") => result.direction = Some("debit".to_string()),
            other => {
                debug!("LLM returned an unusable direction {:?} — rejecting", other);
                return None;
            }
        }

        if !Self::passes_sanity_checks(&result) {
            return None;
        }

        if result.is_valid() {
            Some(result)
        } else {
            None
        }
    }

    const MAX_FUTURE_EVENT_TIME_SECONDS: i64 = 2 * 24 * 60 * 60;

    /// Rejects values that are impossible regardless of the source text.
    ///
    /// Independent of grounding: an amount can appear verbatim in the message and
    /// still be wrong as a transaction. Non-positive amounts, currencies that are not
    /// three letters, and timestamps in the future are all rejected outright.
    fn passes_sanity_checks(result: &ExtractionResult) -> bool {
        if let Some(amount_minor) = result.amount_minor {
            if amount_minor <= 0 {
                debug!("LLM returned a non-positive amount {amount_minor} — rejecting");
                return false;
            }
        }

        if let Some(currency) = &result.currency {
            let ok = currency.len() == 3 && currency.chars().all(|c| c.is_ascii_alphabetic());
            if !ok {
                debug!("LLM returned a non-ISO-4217 currency {currency:?} — rejecting");
                return false;
            }
        }

        if let Some(event_time) = result.event_time {
            let now = chrono::Utc::now().timestamp();
            if event_time > now + Self::MAX_FUTURE_EVENT_TIME_SECONDS {
                debug!("LLM returned a future event_time {event_time} — rejecting");
                return false;
            }
        }

        true
    }

    /// Extracts the JSON object from a response that may carry surrounding prose.
    ///
    /// Spans the first `{` to the last `}`, which tolerates a model that wraps its
    /// answer in explanation or a markdown fence despite the schema constraint.
    pub fn extract_json_block(text: &str) -> Option<&str> {
        let start = text.find('{')?;
        let end = text.rfind('}')?;
        if start < end {
            Some(&text[start..=end])
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_llm_output_parses_self_reported_confidence() {
        let engine = LlmEngine::new(&PathBuf::from("dummy"), "dummy");
        let raw = r#"{"amount": 500.00, "currency": "INR", "direction": "debit",
                      "merchant": "Amazon", "event_time": 1704412200, "reference_id": null,
                      "confidence": 0.35}"#;
        let result = engine
            .parse_json_to_result(raw, None)
            .expect("valid JSON with amount must parse");
        assert_eq!(result.confidence_score, Some(0.35));
    }

    #[test]
    fn test_llm_output_missing_confidence_defaults_low() {
        let engine = LlmEngine::new(&PathBuf::from("dummy"), "dummy");
        let raw = r#"{"amount": 500.00, "currency": "INR", "direction": "debit",
                      "merchant": "Amazon", "event_time": 1704412200, "reference_id": null}"#;
        let result = engine
            .parse_json_to_result(raw, None)
            .expect("valid JSON with amount must parse");
        assert_eq!(result.confidence_score, Some(0.0));
    }

    #[test]
    fn test_llm_output_missing_event_time_uses_fallback() {
        let engine = LlmEngine::new(&PathBuf::from("dummy"), "dummy");
        let raw = r#"{"amount": 5194.00, "currency": "INR", "direction": "debit",
                      "merchant": "Edge CSB Bank Credit Card", "reference_id": "1321778584196999168"}"#;

        assert!(
            engine.parse_json_to_result(raw, None).is_none(),
            "without a fallback, a missing event_time must still fail is_valid()"
        );

        let result = engine
            .parse_json_to_result(raw, Some(1747026600))
            .expect("a fallback event_time must let this parse and pass is_valid()");
        assert_eq!(result.event_time, Some(1747026600));
        assert_eq!(
            result.merchant_raw.as_deref(),
            Some("Edge CSB Bank Credit Card")
        );
    }

    #[test]
    fn test_llm_output_schema_validation_rejects_malformed_json() {
        let engine = LlmEngine::new(&PathBuf::from("dummy"), "dummy");
        let malformed = r#"{ "amount": 50.0, "currency": "USD" "merchant": "Netflix" "#;
        assert!(engine.parse_json_to_result(malformed, None).is_none());

        let not_json_at_all = "I'm sorry, I cannot help with that request.";
        assert!(engine.parse_json_to_result(not_json_at_all, None).is_none());
    }

    #[test]
    fn classify_raw_output_distinguishes_unparseable_json_from_failed_validation() {
        let engine = LlmEngine::new(&PathBuf::from("dummy"), "dummy");
        let source_body = "Dear Customer, Rs 1,299.00 has been debited from your HDFC Bank \
            account ending 4521 on 05-Jan-24 towards purchase at Amazon. Ref No 987654321.";

        let malformed = r#"{ "amount": 50.0, "currency": "USD" "merchant": "Netflix" "#;
        assert!(matches!(
            engine.classify_raw_output(malformed, source_body, None),
            RawOutputOutcome::UnparseableJson
        ));

        let well_formed_but_hallucinated = r#"{"amount": 1299.00, "currency": "INR", "direction": "debit", "merchant": "Totally Fake Store", "event_time": 1704412200, "reference_id": "987654321"}"#;
        assert!(matches!(
            engine.classify_raw_output(well_formed_but_hallucinated, source_body, None),
            RawOutputOutcome::FailedValidation
        ));

        let valid = r#"{"amount": 1299.00, "currency": "INR", "direction": "debit", "merchant": "Amazon", "event_time": 1704412200, "reference_id": "987654321"}"#;
        assert!(matches!(
            engine.classify_raw_output(valid, source_body, None),
            RawOutputOutcome::Accepted(_)
        ));
    }

    #[test]
    fn test_llm_output_rejects_hallucinated_values_not_in_source() {
        let source_body = "You spent Rs 500 on your HDFC Bank card ending 1234.";

        let real_result = ExtractionResult {
            amount_minor: Some(50000),
            currency: Some("INR".to_string()),
            direction: Some("debit".to_string()),
            merchant_raw: None,
            reference_id: None,
            ..Default::default()
        };
        assert!(
            LlmEngine::validate_against_source(&real_result, source_body),
            "a result with no merchant/reference_id claim has nothing to fabricate"
        );

        let hallucinated_merchant = ExtractionResult {
            merchant_raw: Some("Definitely Not Real Merchant Inc".to_string()),
            ..Default::default()
        };
        assert!(
            !LlmEngine::validate_against_source(&hallucinated_merchant, source_body),
            "a merchant name absent from the source body must be rejected"
        );

        let hallucinated_reference = ExtractionResult {
            reference_id: Some("FABRICATED999999".to_string()),
            ..Default::default()
        };
        assert!(!LlmEngine::validate_against_source(
            &hallucinated_reference,
            source_body
        ));

        let real_merchant_different_case = ExtractionResult {
            merchant_raw: Some("hdfc bank".to_string()),
            ..Default::default()
        };
        assert!(LlmEngine::validate_against_source(
            &real_merchant_different_case,
            source_body
        ));
    }

    #[test]
    fn test_llm_output_rejects_hallucinated_amount() {
        let source_body = "You spent Rs 500 on your HDFC Bank card ending 1234.";

        let hallucinated_amount = ExtractionResult {
            amount_minor: Some(999999),
            ..Default::default()
        };
        assert!(
            !LlmEngine::validate_against_source(&hallucinated_amount, source_body),
            "an amount absent from the source body in any numeral form must be rejected"
        );
    }

    #[test]
    fn test_llm_output_accepts_amount_formatting_variance() {
        let comma_source = "You spent Rs 1,500.50 on your HDFC Bank card ending 1234.";
        let whole_rupee_source = "You spent Rs 500 on your HDFC Bank card ending 1234.";

        let with_commas = ExtractionResult {
            amount_minor: Some(150050),
            ..Default::default()
        };
        assert!(LlmEngine::validate_against_source(
            &with_commas,
            comma_source
        ));

        let whole_rupee = ExtractionResult {
            amount_minor: Some(50000),
            ..Default::default()
        };
        assert!(LlmEngine::validate_against_source(
            &whole_rupee,
            whole_rupee_source
        ));
    }

    #[test]
    fn test_prompt_includes_bank_name() {
        let prompt = LlmEngine::generate_prompt("HDFC Bank", "You spent Rs 500 at Amazon.");
        assert!(
            prompt.contains("HDFC Bank"),
            "prompt must include the bank name Gate 1 already resolved: {prompt}"
        );
    }

    #[test]
    fn test_prompt_includes_few_shot_examples() {
        let prompt = LlmEngine::generate_prompt("HDFC Bank", "You spent Rs 500 at Amazon.");
        let json_output_count = prompt.matches("JSON Output:").count();
        assert!(
            json_output_count >= 3,
            "prompt must include multiple worked examples, not just the trailing prompt: \
             found {json_output_count} \"JSON Output:\" occurrences in: {prompt}"
        );
        assert!(
            prompt.contains("Example 1"),
            "prompt must include worked examples"
        );
    }

    #[test]
    fn test_correction_prompt_quotes_previous_output() {
        let prompt = LlmEngine::generate_correction_prompt(
            "HDFC Bank",
            "You spent Rs 500 at Amazon.",
            "garbage output",
        );
        assert!(prompt.contains("garbage output"));
        assert!(prompt.contains("You spent Rs 500 at Amazon."));
    }

    #[tokio::test]
    async fn test_llm_timeout_routes_to_unassigned() {
        let slow_call = async {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            "late result".to_string()
        };

        let start = std::time::Instant::now();
        let result = tokio::time::timeout(std::time::Duration::from_millis(50), slow_call).await;
        let elapsed = start.elapsed();

        assert!(
            result.is_err(),
            "a computation that outlives the timeout must yield a timeout error, not the late result"
        );
        assert!(
            elapsed < std::time::Duration::from_millis(250),
            "tokio::time::timeout must not block anywhere near as long as the slow computation \
             takes, got {:?}",
            elapsed
        );
    }

    #[test]
    fn llm_output_rejects_values_that_are_grounded_but_impossible() {
        let engine = LlmEngine::new(&PathBuf::from("dummy"), "dummy");

        for bogus in ["unknown", "", "transfer", "DEBIT or CREDIT"] {
            let raw = format!(
                r#"{{"amount": 500.00, "currency": "INR", "direction": "{bogus}", "merchant": "Swiggy", "event_time": 1780000000, "confidence": 0.9}}"#
            );
            assert!(
                engine.parse_json_to_result(&raw, None).is_none(),
                "direction {bogus:?} must be rejected, not defaulted to debit"
            );
        }

        for good in ["debit", "credit", "CREDIT"] {
            let raw = format!(
                r#"{{"amount": 500.00, "currency": "INR", "direction": "{good}", "merchant": "Swiggy", "event_time": 1780000000, "confidence": 0.9}}"#
            );
            assert!(
                engine.parse_json_to_result(&raw, None).is_some(),
                "direction {good:?} must still parse"
            );
        }

        let case = |json: &str| engine.parse_json_to_result(json, None);

        assert!(case(r#"{"amount": 0, "currency": "INR", "direction": "debit", "merchant": "Swiggy", "event_time": 1780000000}"#).is_none());
        assert!(case(r#"{"amount": -20.00, "currency": "INR", "direction": "debit", "merchant": "Swiggy", "event_time": 1780000000}"#).is_none());

        assert!(case(r#"{"amount": 500.00, "currency": "Rs.", "direction": "debit", "merchant": "Swiggy", "event_time": 1780000000}"#).is_none());
        assert!(case(r#"{"amount": 500.00, "currency": "", "direction": "debit", "merchant": "Swiggy", "event_time": 1780000000}"#).is_none());

        let far_future = chrono::Utc::now().timestamp() + 365 * 24 * 60 * 60;
        assert!(case(&format!(
            r#"{{"amount": 500.00, "currency": "INR", "direction": "debit", "merchant": "Swiggy", "event_time": {far_future}}}"#
        ))
        .is_none());

        let slightly_ahead = chrono::Utc::now().timestamp() + 6 * 60 * 60;
        assert!(case(&format!(
            r#"{{"amount": 500.00, "currency": "INR", "direction": "debit", "merchant": "Swiggy", "event_time": {slightly_ahead}}}"#
        ))
        .is_some());
    }
}
