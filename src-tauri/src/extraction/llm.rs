use super::ladder::ExtractionResult;
use crate::logging::llm_logger::{LlmCallContext, LlmCallType};
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

/// Result of a Layer 6 attempt, distinguishing a wall-clock timeout (worth
/// retrying — see `extract`'s doc comment) from every other failure mode.
#[derive(Debug, Clone, PartialEq)]
pub enum Layer6Outcome {
    Extracted(Box<ExtractionResult>),
    TimedOut,
    /// Infra-level failure (no model downloaded, sidecar unreachable) --
    /// tells us nothing about whether the email is a real transaction, so
    /// it must stay retriable rather than ever being treated as terminal.
    Failed,
    /// The model produced a response on both attempts (including the
    /// self-correction retry) but it never parsed/validated -- i.e. Layer 6
    /// actually looked at this email and confirmed there's no extractable
    /// transaction in it (a misclassified marketing/notification email, most
    /// commonly). Distinct from `Failed` so the caller can treat this as a
    /// terminal, non-retriable "not a transaction" result instead of leaving
    /// the item stuck in the review queue forever.
    Rejected,
}

/// Internal to `LlmEngine::run_completion` -- one sidecar call's raw outcome,
/// before `extract` decides whether a `Rejected` attempt gets a
/// self-correction retry (spec optimization #3).
enum CompletionAttempt {
    Extracted(Box<ExtractionResult>),
    TimedOut,
    /// Sidecar responded but the output was unparseable JSON or failed
    /// `validate_against_source` -- carries the raw text so a correction
    /// prompt can quote it back to the model.
    Rejected(String),
    InfraFailed,
}

/// What a raw sidecar completion turned out to be, once parsed and
/// validated -- split out from `run_completion`'s match arms (Doc
/// 2026-07-28 dev-scan-log-issues) so a JSON-parse failure and a
/// `validate_against_source` failure log distinguishably, instead of both
/// collapsing into the same "value not present in source text" message,
/// which made it impossible to tell from logs which one was actually
/// happening.
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

    /// Spec optimization #3's self-correction loop: quotes the model's own
    /// rejected output back to it along with a concrete complaint, rather
    /// than re-asking the exact same zero-shot question a second time.
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

    /// Evaluates `prompt` against `llama-server`. Retries transient
    /// (server-starting) errors up to 120s; returns `TimedOut` if the server
    /// is up but the prompt inference itself takes longer than the sidecar's
    /// own calibrated timeout (`llama_sidecar::calibrate_timeout`). Returns
    /// `Failed` on non-recoverable errors (model missing, parse failed,
    /// hardware incompatible).
    ///
    /// Never blocks the main `historical_scan` loop — this async function
    /// yields to the Tokio runtime while waiting, so other concurrent
    /// fetches proceed while this one waits for `llama-server` or until its
    /// completion semaphore is free next time.
    /// `fallback_event_time` (Gmail's `internalDate`, already resolved by the
    /// caller) fills in for the model's self-reported `event_time` when it
    /// omits that field -- which the JSON-schema grammar sent to `llama-server`
    /// allows it to do (no `required` list) and which it does on essentially
    /// every real call, since bank emails rarely state the transaction time as
    /// a Unix timestamp the model could copy verbatim. Without a fallback here,
    /// `ExtractionResult::is_valid()`'s unconditional `event_time.is_some()`
    /// check rejects an otherwise-correct extraction as "unparseable JSON".
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

    /// One prompt -> one sidecar call -> one classified outcome. Factored out
    /// of `extract` so the self-correction retry (spec optimization #3) can
    /// reuse the exact same timeout/backoff/parse/validate logic instead of
    /// duplicating it.
    ///
    /// `attempt` is 1 for the first try and 2 for the self-correction retry.
    /// It is stamped on every `llm_calls.log` entry so retry patterns are
    /// distinguishable without post-processing.
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

    /// Parses the raw text output from the LLM, extracting JSON and converting
    /// to ExtractionResult. `fallback_event_time` fills in for the model's
    /// `event_time` when it's absent from the JSON -- see `extract`'s doc
    /// comment for why that's the normal case, not an edge case.
    pub fn parse_json_to_result(
        &self,
        llm_output: &str,
        fallback_event_time: Option<i64>,
    ) -> Option<ExtractionResult> {
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
            // Self-reported by the model (Doc 12 §6.3 revision, 2026-07-30):
            // a missing field is treated as zero confidence, not the old
            // fixed 0.7 -- absence of a self-report is itself a signal the
            // model didn't engage with the confidence instruction, which
            // must not read as "fully sure."
            confidence_score: Some(parsed.confidence.unwrap_or(0.0).clamp(0.0, 1.0)),
            amount_minor: parsed.amount.map(|v| (v * 100.0).round() as i64),
            currency: parsed.currency,
            direction: parsed.direction,
            merchant_raw: parsed.merchant,
            event_time: parsed.event_time.or(fallback_event_time),
            reference_id: parsed.reference_id,
            ..Default::default()
        };

        // audit_06 #10: this used to read `if dir == "credit" { credit } else
        // { debit }` — so *anything* the model returned that wasn't the exact
        // word "credit" became a confident debit. "unknown", "", "transfer",
        // a hallucinated sentence: all silently booked as money leaving the
        // user's account. Direction has no safe default; an unrecognised one
        // is a rejected extraction.
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

    /// The furthest ahead of "now" an extracted `event_time` may sit before it
    /// is treated as fabricated. Not a tuning knob: a bank alerts you *after*
    /// a transaction, so the only legitimate future offsets are timezone
    /// spread (max ~26h) and clock skew. Two days covers both with room over.
    const MAX_FUTURE_EVENT_TIME_SECONDS: i64 = 2 * 24 * 60 * 60;

    /// audit_06 #10: `validate_against_source` only asks whether each value
    /// *appears somewhere* in the email. That catches invention but not
    /// nonsense — an amount of zero, a currency of `"Rs."`, or a date in 2087
    /// can all be grounded in the source text and still be wrong.
    ///
    /// These are schema and range facts, not thresholds: each one rejects a
    /// value that could not be correct under any reading of the email.
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

    /// Self-reported confidence (2026-07-30, replacing the fixed 0.7): the
    /// model's own stated uncertainty must be carried through verbatim, not
    /// discarded in favor of a constant.
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

    /// A model that omits the field entirely must not be treated as
    /// confident by default -- absence of a self-report is itself a
    /// low-confidence signal, not evidence of certainty.
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

    /// Regression test for a 100% Layer 6 failure rate observed in
    /// production (2026-07-30 root-cause analysis of the Unassigned queue):
    /// the model's JSON schema/grammar has no `required` list, so every
    /// real call omits `event_time`, and `ExtractionResult::is_valid()`
    /// unconditionally required it -- rejecting an otherwise-correct
    /// extraction as "unparseable JSON" every single time. A caller-supplied
    /// fallback (Gmail's `internalDate`) must fill the gap.
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

    /// Doc 30 TASK-TXN-006 acceptance test.
    #[test]
    fn test_llm_output_schema_validation_rejects_malformed_json() {
        let engine = LlmEngine::new(&PathBuf::from("dummy"), "dummy");
        let malformed = r#"{ "amount": 50.0, "currency": "USD" "merchant": "Netflix" "#;
        assert!(engine.parse_json_to_result(malformed, None).is_none());

        let not_json_at_all = "I'm sorry, I cannot help with that request.";
        assert!(engine.parse_json_to_result(not_json_at_all, None).is_none());
    }

    /// Doc 2026-07-28 dev-scan-log-issues: `run_completion` previously
    /// logged the same "value not present in source text" message whether
    /// the raw output was unparseable JSON or well-formed JSON that failed
    /// `validate_against_source` -- two different problems that were
    /// indistinguishable in logs. `classify_raw_output` is what the fix
    /// hangs off of; this proves the three outcomes are actually
    /// distinguished.
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

    /// Spec optimization #3: the prompt must carry worked examples, not just
    /// a bare field-list instruction -- cheap structural proof the few-shot
    /// block is actually present (multiple "JSON Output:" occurrences: the
    /// examples plus the real trailing prompt).
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

    /// Spec optimization #3's self-correction loop: the correction prompt
    /// must quote the model's own rejected output back to it, and still
    /// include the original email body to re-ground the retry.
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

    /// audit_06 #10: `validate_against_source` only asks whether a value
    /// appears somewhere in the email, which catches invention but not
    /// nonsense. Each case here is a value that is grounded in the source (or
    /// needs no grounding) and still cannot be correct.
    #[test]
    fn llm_output_rejects_values_that_are_grounded_but_impossible() {
        let engine = LlmEngine::new(&PathBuf::from("dummy"), "dummy");

        // The bug this replaced: anything that wasn't the literal word
        // "credit" became a confident *debit*. A model that says it doesn't
        // know must not have that read as money leaving the account.
        for bogus in ["unknown", "", "transfer", "DEBIT or CREDIT"] {
            let raw = format!(
                r#"{{"amount": 500.00, "currency": "INR", "direction": "{bogus}", "merchant": "Swiggy", "event_time": 1780000000, "confidence": 0.9}}"#
            );
            assert!(
                engine.parse_json_to_result(&raw, None).is_none(),
                "direction {bogus:?} must be rejected, not defaulted to debit"
            );
        }

        // Both real directions still parse.
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

        // Zero and negative amounts are not transactions.
        assert!(case(r#"{"amount": 0, "currency": "INR", "direction": "debit", "merchant": "Swiggy", "event_time": 1780000000}"#).is_none());
        assert!(case(r#"{"amount": -20.00, "currency": "INR", "direction": "debit", "merchant": "Swiggy", "event_time": 1780000000}"#).is_none());

        // A currency has to be an ISO-4217-shaped code -- "Rs." is printed in
        // the email, so a substring check would happily accept it.
        assert!(case(r#"{"amount": 500.00, "currency": "Rs.", "direction": "debit", "merchant": "Swiggy", "event_time": 1780000000}"#).is_none());
        assert!(case(r#"{"amount": 500.00, "currency": "", "direction": "debit", "merchant": "Swiggy", "event_time": 1780000000}"#).is_none());

        // A bank alerts you after the fact; it cannot report next decade.
        let far_future = chrono::Utc::now().timestamp() + 365 * 24 * 60 * 60;
        assert!(case(&format!(
            r#"{{"amount": 500.00, "currency": "INR", "direction": "debit", "merchant": "Swiggy", "event_time": {far_future}}}"#
        ))
        .is_none());

        // ...but a transaction dated a few hours ahead (timezone spread,
        // clock skew) is ordinary and must survive.
        let slightly_ahead = chrono::Utc::now().timestamp() + 6 * 60 * 60;
        assert!(case(&format!(
            r#"{{"amount": 500.00, "currency": "INR", "direction": "debit", "merchant": "Swiggy", "event_time": {slightly_ahead}}}"#
        ))
        .is_some());
    }
}
