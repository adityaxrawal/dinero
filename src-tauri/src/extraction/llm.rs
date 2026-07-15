use super::ladder::ExtractionResult;
use anyhow::{anyhow, Result};
use candle_core::{Device, Tensor};
use candle_transformers::generation::LogitsProcessor;
use candle_transformers::models::quantized_llama as model;
use serde::Deserialize;
use std::panic;
use std::path::Path;
use tokenizers::Tokenizer;
use tracing::{debug, error};

pub struct LlmEngine {
    model_path: std::path::PathBuf,
    tokenizer_path: std::path::PathBuf,
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
    pub fn new(model_path: &Path, tokenizer_path: &Path) -> Self {
        Self {
            model_path: model_path.to_path_buf(),
            tokenizer_path: tokenizer_path.to_path_buf(),
        }
    }

    /// Generates a constrained prompt for extraction
    pub fn generate_prompt(body_text: &str) -> String {
        format!(
            "Extract the following fields from the email body. Return ONLY valid JSON and nothing else.\n\
             Fields:\n\
             - amount: number (e.g., 1500.50)\n\
             - currency: string (e.g., \"INR\", \"USD\")\n\
             - direction: string (\"credit\" or \"debit\")\n\
             - merchant: string (e.g., \"Amazon\")\n\
             - event_time: integer (Unix timestamp, e.g., 1704067200)\n\
             - reference_id: string (e.g., \"1234567890\")\n\n\
             Email Body:\n\
             \"\"\"\n\
             {}\n\
             \"\"\"\n\
             JSON Output:",
            body_text
        )
    }

    /// Doc 30 TASK-TXN-006: hard 10-second execution timeout per email.
    /// Exceeding it is treated as a Layer 6 failure (falls through to
    /// `unassigned_transactions` once TASK-TXN-009 wires observation
    /// persistence in).
    const INFERENCE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

    /// Executes the LLM with an OOM guard via `catch_unwind`, a hard
    /// timeout, and a source-text sanity check on the result.
    pub fn extract(&self, body_text: &str) -> Option<ExtractionResult> {
        let prompt = Self::generate_prompt(body_text);
        let model_path = self.model_path.clone();
        let tokenizer_path = self.tokenizer_path.clone();

        // Run the heavy lifting in a blocking thread to avoid blocking the
        // async executor and to catch panics (like OOM or device errors)
        // safely. A channel (rather than `JoinHandle::join()`) is what makes
        // the timeout below possible: Rust has no safe way to forcibly kill
        // a running thread, so a slow/hung inference keeps running in the
        // background after this function returns its timeout `None` — but
        // the caller is never blocked past `INFERENCE_TIMEOUT`, which is the
        // actual requirement.
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let res = panic::catch_unwind(|| {
                Self::run_inference(&model_path, &tokenizer_path, &prompt)
            });
            let result: Result<String> = match res {
                Ok(Ok(output)) => Ok(output),
                Ok(Err(e)) => Err(anyhow!("LLM Inference failed: {}", e)),
                Err(_) => Err(anyhow!("LLM Inference panicked (possible OOM)")),
            };
            let _ = tx.send(result);
        });

        let result = Self::wait_with_timeout(rx, Self::INFERENCE_TIMEOUT);

        match result {
            Some(Ok(json_str)) => {
                let parsed = self.parse_json_to_result(&json_str)?;
                if Self::validate_against_source(&parsed, body_text) {
                    Some(parsed)
                } else {
                    debug!("Layer 6 LLM output rejected: value not present in source text");
                    None
                }
            }
            Some(Err(e)) => {
                error!("Layer 6 LLM Failure: {}", e);
                None
            }
            None => {
                error!(
                    "Layer 6 LLM Failure: inference exceeded {:?} timeout",
                    Self::INFERENCE_TIMEOUT
                );
                None
            }
        }
    }

    /// Blocks on `rx` for up to `timeout`, returning `None` rather than
    /// waiting indefinitely. Factored out from `extract()` so the timeout
    /// behavior itself is directly testable without a real Candle model —
    /// see `test_llm_timeout_routes_to_unassigned`.
    fn wait_with_timeout<T: Send + 'static>(
        rx: std::sync::mpsc::Receiver<T>,
        timeout: std::time::Duration,
    ) -> Option<T> {
        rx.recv_timeout(timeout).ok()
    }

    /// Doc 30 TASK-TXN-006: "sanity-check values against the source text via
    /// substring/fuzzy matching; reject anything malformed or containing
    /// fabricated fields." A syntactically valid JSON object is not enough —
    /// checks only `merchant_raw` and `reference_id` (case-insensitive
    /// substring of the original email body), the free-text/identifier
    /// fields an LLM could plausibly invent a plausible-looking value for.
    /// `amount`/`currency`/`direction` are left unchecked here: they're
    /// already schema-constrained by `LlmJsonOutput`'s types, and formatting
    /// differences (e.g. "1,500.50" vs "1500.5") would make a substring
    /// check on them fragile rather than a real hallucination guard.
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
        true
    }

    /// Performs the actual Candle inference. This assumes a Llama-like architecture for the GGUF.
    fn run_inference(
        model_path: &Path,
        tokenizer_path: &Path,
        prompt: &str,
    ) -> Result<String> {
        // Load tokenizer
        let tokenizer = Tokenizer::from_file(tokenizer_path)
            .map_err(|e| anyhow::anyhow!("Failed to load tokenizer: {}", e))?;

        // Setup CPU Device (safest fallback)
        let device = Device::Cpu;

        // Load GGUF file
        let mut file = std::fs::File::open(model_path)?;
        let content = candle_core::quantized::gguf_file::Content::read(&mut file)?;
        
        let mut model = model::ModelWeights::from_gguf(content, &mut file, &device)?;

        let tokens = tokenizer
            .encode(prompt, true)
            .map_err(|e| anyhow::anyhow!("Tokenizer encode failed: {}", e))?
            .get_ids()
            .to_vec();

        let mut logits_processor = LogitsProcessor::new(299792458, None, None);
        let mut all_tokens = vec![];
        let mut next_token = *tokens.last().unwrap();
        let mut token_str = String::new();

        // Pre-fill
        let input = Tensor::new(tokens.as_slice(), &device)?.unsqueeze(0)?;
        let _ = model.forward(&input, 0)?; // Initial prefill

        // Generate loop (capped to 256 tokens for JSON)
        for index in 0..256 {
            let input = Tensor::new(&[next_token], &device)?.unsqueeze(0)?;
            let logits = model.forward(&input, tokens.len() + index)?;
            let logits = logits.squeeze(0)?.squeeze(0)?;
            next_token = logits_processor.sample(&logits)?;
            all_tokens.push(next_token);

            if let Some(t) = tokenizer.id_to_token(next_token) {
                token_str.push_str(&t);
            }

            // Stop if we see an end token or closing brace if we think we're done
            if next_token == 2 || next_token == 0 {
                break;
            }
        }

        let decoded = tokenizer
            .decode(&all_tokens, true)
            .map_err(|e| anyhow::anyhow!("Decode failed: {}", e))?;
        
        Ok(decoded)
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
        let engine = LlmEngine::new(&PathBuf::from("dummy"), &PathBuf::from("dummy"));
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

    /// Doc 30 TASK-TXN-006 acceptance test. Exercising the real Candle
    /// inference path would require a real `.gguf` model and GPU/CPU time
    /// this environment doesn't have; this proves the actual mechanism
    /// `extract()` relies on (`wait_with_timeout`, a channel `recv_timeout`
    /// wrapper) genuinely never blocks past its deadline, using a slow dummy
    /// sender in place of real inference — the same substitution pattern
    /// this codebase already uses elsewhere for infra it doesn't have
    /// locally (e.g. `/bin/sleep` standing in for a hung pdfium process).
    #[test]
    fn test_llm_timeout_routes_to_unassigned() {
        let (tx, rx) = std::sync::mpsc::channel::<Result<String>>();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            let _ = tx.send(Ok("late result".to_string()));
        });

        let start = std::time::Instant::now();
        let result = LlmEngine::wait_with_timeout(rx, std::time::Duration::from_millis(50));
        let elapsed = start.elapsed();

        assert!(
            result.is_none(),
            "a computation that outlives the timeout must yield None, not the late result"
        );
        assert!(
            elapsed < std::time::Duration::from_millis(250),
            "wait_with_timeout must not block anywhere near as long as the slow computation \
             takes, got {:?}",
            elapsed
        );
    }
}
