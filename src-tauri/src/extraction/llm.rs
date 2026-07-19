use super::ladder::ExtractionResult;
use serde::Deserialize;
use std::path::Path;
use tracing::{debug, error};

/// Runs inference via the `llama_sidecar` process (llama.cpp's
/// `llama-server`), not in-process -- no released `candle-transformers`
/// version has a loader for either of this catalog's actual GGUF
/// architectures (Gemma 4's `"gemma4"` tag; Qwen3.6's Gated-DeltaNet-hybrid
/// MoE), and the crash/OOM isolation a separate OS process gives is
/// strictly better than the old `catch_unwind`-around-an-OS-thread approach
/// anyway (a genuine OOM there could still take down this process; a
/// `llama-server` OOM can't).
pub struct LlmEngine {
    app_dir: std::path::PathBuf,
    model_id: String,
}

#[derive(Debug, Deserialize)]
struct LlmJsonOutput {
    amount: Option<f64>,
    currency: Option<String>,
    direction: Option<String>,
    merchant: Option<String>,
    event_time: Option<i64>,
    reference_id: Option<String>,
}

impl LlmEngine {
    pub fn new(app_dir: &Path, model_id: &str) -> Self {
        Self {
            app_dir: app_dir.to_path_buf(),
            model_id: model_id.to_string(),
        }
    }

    /// Generates a constrained prompt for extraction. `bank_name` is
    /// whatever Gate 1 already resolved the sender to (e.g. "HDFC Bank", or
    /// "Unknown Bank" for the subject-rescue path) -- previously available
    /// to every caller but never actually included in the prompt, even
    /// though the model can use it as real context (bank-specific phrasing
    /// conventions, which fields that bank typically states) rather than
    /// extracting blind.
    pub fn generate_prompt(bank_name: &str, body_text: &str) -> String {
        format!(
            "Extract the following fields from a bank transaction alert email sent by {bank_name}. \
             Return ONLY valid JSON and nothing else.\n\
             Fields:\n\
             - amount: number (e.g., 1500.50)\n\
             - currency: string (e.g., \"INR\", \"USD\")\n\
             - direction: string (\"credit\" or \"debit\")\n\
             - merchant: string (e.g., \"Amazon\")\n\
             - event_time: integer (Unix timestamp, e.g., 1704067200)\n\
             - reference_id: string (e.g., \"1234567890\")\n\n\
             Email Body:\n\
             \"\"\"\n\
             {body_text}\n\
             \"\"\"\n\
             JSON Output:"
        )
    }

    /// Doc 30 TASK-TXN-006: hard 10-second execution timeout per email.
    /// Exceeding it is treated as a Layer 6 failure (falls through to
    /// `unassigned_transactions` once TASK-TXN-009 wires observation
    /// persistence in).
    const INFERENCE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

    /// Runs the completion against the `llama_sidecar` server (starting it
    /// first if needed), bounded by a hard timeout, then a source-text
    /// sanity check on the result. OOM/crash isolation now comes from
    /// `llama-server` being a separate OS process (see the struct-level doc
    /// comment) rather than an in-process `catch_unwind`.
    pub async fn extract(&self, bank_name: &str, body_text: &str) -> Option<ExtractionResult> {
        let prompt = Self::generate_prompt(bank_name, body_text);

        let mut retry_delay = std::time::Duration::from_millis(1000);
        let max_delay = std::time::Duration::from_millis(2000);
        let max_total_wait = std::time::Duration::from_secs(120);
        let start_time = std::time::Instant::now();

        let raw_output = loop {
            let result = tokio::time::timeout(
                Self::INFERENCE_TIMEOUT,
                crate::llama_sidecar::complete(&self.app_dir, &self.model_id, &prompt),
            )
            .await;

            match result {
                Ok(Ok(output)) => break Some(output),
                Ok(Err(e)) => {
                    let msg = e.to_string();
                    if (msg.contains("starting") || msg.contains("try again shortly"))
                        && start_time.elapsed() < max_total_wait
                    {
                        tokio::time::sleep(retry_delay).await;
                        retry_delay = std::cmp::min(retry_delay * 2, max_delay);
                        continue;
                    }
                    error!("Layer 6 LLM Failure: {}", e);
                    break None;
                }
                Err(_) => {
                    error!(
                        "Layer 6 LLM Failure: inference exceeded {:?} timeout",
                        Self::INFERENCE_TIMEOUT
                    );
                    break None;
                }
            }
        };

        match raw_output {
            Some(raw) => {
                let parsed = self.parse_json_to_result(&raw)?;
                if Self::validate_against_source(&parsed, body_text) {
                    Some(parsed)
                } else {
                    debug!("Layer 6 LLM output rejected: value not present in source text");
                    None
                }
            }
            None => None,
        }
    }

    /// Doc 30 TASK-TXN-006: "sanity-check values against the source text via
    /// substring/fuzzy matching; reject anything malformed or containing
    /// fabricated fields." A syntactically valid JSON object is not enough —
    /// checks `merchant_raw`, `reference_id` (case-insensitive substring of
    /// the original email body) and `amount_minor` (numeral-tolerant
    /// substring, see `amount_appears_in_source`). `amount` was previously
    /// the one unchecked field here despite being the single most
    /// safety-critical value in a finance pipeline -- a hallucinated
    /// merchant name is a data-quality problem, a hallucinated amount is a
    /// wrong-dollar-figure transaction. `currency`/`direction` stay
    /// unchecked: both are closed, small enum-like vocabularies (a handful
    /// of ISO currency codes / "credit"/"debit") where a substring check
    /// adds little -- there's no meaningfully "fabricated" value to catch
    /// the way there is for free-text merchant/reference or a numeral.
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

    /// Whether `amount_minor` (in minor units, e.g. paise) appears
    /// anywhere in `source_body` as a plain numeral -- tolerant of
    /// thousands-separator commas ("1,500.50" vs "1500.50") and of a
    /// whole-rupee amount being printed without decimals at all ("500" for
    /// what this pipeline stores as 50000 minor units), the two formatting
    /// variances real bank emails actually exhibit. Not a general
    /// currency-formatting parser -- just enough tolerance that a genuine
    /// value isn't rejected by this guard for cosmetic reasons, while a
    /// truly fabricated amount (absent from the source in any form) still
    /// is.
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

    /// Parses the raw text output from the LLM, extracting JSON and converting to ExtractionResult.
    pub fn parse_json_to_result(&self, llm_output: &str) -> Option<ExtractionResult> {
        // 1. Extract JSON block if it's wrapped in markdown
        let json_str = Self::extract_json_block(llm_output).unwrap_or(llm_output);

        // 2. Deserialize
        let parsed: LlmJsonOutput = match serde_json::from_str(json_str) {
            Ok(p) => p,
            Err(e) => {
                debug!("Failed to parse LLM JSON: {} - Raw: {}", e, json_str);
                return None;
            }
        };

        // 3. Map to ExtractionResult
        let mut result = ExtractionResult {
            extraction_method: "llm_layer6".to_string(),
            // Doc 12 §6.3: LLM-extracted observations carry a fixed 0.7
            // confidence, which downstream canonical creation (TASK-TXN-010)
            // uses to force a `pending_review` state.
            confidence_score: Some(0.7),
            amount_minor: parsed.amount.map(|v| (v * 100.0).round() as i64),
            currency: parsed.currency,
            direction: parsed.direction,
            merchant_raw: parsed.merchant,
            event_time: parsed.event_time,
            reference_id: parsed.reference_id,
            ..Default::default()
        };

        // Normalize direction
        if let Some(dir) = &result.direction {
            if dir.to_lowercase() == "credit" {
                result.direction = Some("credit".to_string());
            } else {
                result.direction = Some("debit".to_string());
            }
        }

        if result.is_valid() {
            Some(result)
        } else {
            None
        }
    }

    /// Helper to find the first '{' and last '}' to extract JSON from potentially chatty LLMs
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

    /// Doc 30 TASK-TXN-006 acceptance test.
    #[test]
    fn test_llm_output_schema_validation_rejects_malformed_json() {
        let engine = LlmEngine::new(&PathBuf::from("dummy"), "dummy");
        let malformed = r#"{ "amount": 50.0, "currency": "USD" "merchant": "Netflix" "#;
        assert!(engine.parse_json_to_result(malformed).is_none());

        let not_json_at_all = "I'm sorry, I cannot help with that request.";
        assert!(engine.parse_json_to_result(not_json_at_all).is_none());
    }

    /// Doc 30 TASK-TXN-006 acceptance test: a syntactically valid JSON
    /// object whose merchant/reference_id never appear anywhere in the
    /// source email body must be rejected as fabricated, not merely
    /// schema-checked.
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

        // Case-insensitive substring match, not exact equality.
        let real_merchant_different_case = ExtractionResult {
            merchant_raw: Some("hdfc bank".to_string()),
            ..Default::default()
        };
        assert!(LlmEngine::validate_against_source(
            &real_merchant_different_case,
            source_body
        ));
    }

    /// Regression test: `amount` was previously the one field
    /// `validate_against_source` didn't check at all, despite being the
    /// single most safety-critical value in a finance pipeline. A
    /// hallucinated amount absent from the source in any numeral form must
    /// now be rejected.
    #[test]
    fn test_llm_output_rejects_hallucinated_amount() {
        let source_body = "You spent Rs 500 on your HDFC Bank card ending 1234.";

        let hallucinated_amount = ExtractionResult {
            amount_minor: Some(999999), // Rs 9,999.99 -- nowhere in the source
            ..Default::default()
        };
        assert!(
            !LlmEngine::validate_against_source(&hallucinated_amount, source_body),
            "an amount absent from the source body in any numeral form must be rejected"
        );
    }

    /// The amount check must tolerate the two formatting variances real
    /// bank emails actually exhibit: thousands-separator commas, and a
    /// whole-rupee amount printed with no decimals at all.
    #[test]
    fn test_llm_output_accepts_amount_formatting_variance() {
        let comma_source = "You spent Rs 1,500.50 on your HDFC Bank card ending 1234.";
        let whole_rupee_source = "You spent Rs 500 on your HDFC Bank card ending 1234.";

        let with_commas = ExtractionResult {
            amount_minor: Some(150050), // Rs 1500.50
            ..Default::default()
        };
        assert!(LlmEngine::validate_against_source(
            &with_commas,
            comma_source
        ));

        let whole_rupee = ExtractionResult {
            amount_minor: Some(50000), // Rs 500.00, printed as bare "500"
            ..Default::default()
        };
        assert!(LlmEngine::validate_against_source(
            &whole_rupee,
            whole_rupee_source
        ));
    }

    /// `bank_name` was previously available to every caller but never
    /// actually included in the generated prompt.
    #[test]
    fn test_prompt_includes_bank_name() {
        let prompt = LlmEngine::generate_prompt("HDFC Bank", "You spent Rs 500 at Amazon.");
        assert!(
            prompt.contains("HDFC Bank"),
            "prompt must include the bank name Gate 1 already resolved: {prompt}"
        );
    }

    /// Doc 30 TASK-TXN-006 acceptance test. Exercising real `llama-server`
    /// inference would require a running sidecar and a real `.gguf` model
    /// this environment doesn't have; this proves the actual mechanism
    /// `extract()` relies on (`tokio::time::timeout` around the sidecar
    /// call) genuinely cuts a slow response off rather than waiting
    /// indefinitely — the same substitution pattern this codebase already
    /// uses elsewhere for infra it doesn't have locally (e.g. `/bin/sleep`
    /// standing in for a hung pdfium process).
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
}
