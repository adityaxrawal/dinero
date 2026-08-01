//! Structured per-call logger for all LLM inference requests.
//!
//! Every invocation of `llama_sidecar::raw_complete` — regardless of caller
//! (Layer 6 extraction, rule authoring, future consumers) — passes through
//! [`log_llm_request`] before the HTTP hop and [`log_llm_response`] after it.
//!
//! ## Log file format — `logs/llm_calls.log.YYYY-MM-DD`
//!
//! Each request/response pair is rendered as a self-contained block:
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │  ▶ LLM REQUEST  [layer6_extraction]  attempt 1/2  2026-07-30 21:30:04  │
//! ├─────────────────────────────────────────────────────────────────────────┤
//! │  Model       : gemma4_e4b                                               │
//! │  Prompt size : 2,805 chars                                              │
//! ├─────────────────────────────────────────────────────────────────────────┤
//! │  PROMPT BODY (first 500 chars):                                         │
//! │  Extract the following fields from a bank transaction alert email …     │
//! └─────────────────────────────────────────────────────────────────────────┘
//!
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │  ◀ LLM RESPONSE [layer6_extraction]  attempt 1/2  2026-07-30 21:30:11  │
//! ├─────────────────────────────────────────────────────────────────────────┤
//! │  Model       : gemma4_e4b                                               │
//! │  Duration    : 6,736 ms                                                 │
//! │  Outcome     : ✓ ACCEPTED                                               │
//! │  Output size : 139 chars                                                │
//! ├─────────────────────────────────────────────────────────────────────────┤
//! │  RAW OUTPUT:                                                            │
//! │  {"amount": 1299.00, "currency": "INR", "direction": "debit", …}       │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! The prompt body and raw output are always written at INFO level (they are the
//! *point* of this file). PII is redacted by the shared `RedactingWriter` pipeline
//! before anything reaches disk, so the content is safe to store.
//!
//! ## Call types
//!
//! `call_type` on every block header:
//! - `layer6_extraction` — email → transaction field extraction (Layer 6)
//! - `rule_authoring`    — user correction → regex rule (learning worker)
//! - `merchant_cleanup`  — merchant name / category resolution
//! - `statement_row_extraction` — PDF statement row parsing
//! - `sidecar`           — calibration warmup / unclassified
//!
//! ## Outcome values
//!
//! - `✓ ACCEPTED`            — parsed JSON passed source validation
//! - `✗ REJECTED (json)`     — model output was not parseable JSON
//! - `✗ REJECTED (validation)` — JSON valid but field values not in source text
//! - `⚠ TIMED OUT`           — inference exceeded calibrated timeout
//! - `✗ INFRA FAILED`        — sidecar down, HTTP error, or OOM

use std::fmt::Write as FmtWrite;
use std::io::Write as IoWrite;

/// How many characters of the prompt / output to write into the block.
/// Full content is captured since this is the dedicated LLM log file.
const PROMPT_PREVIEW_CHARS: usize = 1000;
const OUTPUT_PREVIEW_CHARS: usize = 1000;
const BOX_WIDTH: usize = 80;

// ─── Public types ─────────────────────────────────────────────────────────────

/// Identifies which subsystem triggered the LLM call, stamped on every log
/// block header. Add new variants here as new LLM consumers are added.
#[derive(Debug, Clone, Copy)]
pub enum LlmCallType {
    /// Layer 6 extraction: extracting transaction fields from an email body.
    Layer6Extraction,
    /// Rule authoring: generating a regex from a user correction.
    RuleAuthoring,
    /// Merchant cleanup / category resolution pass.
    MerchantCleanup,
    /// Statement PDF row extraction.
    StatementRowExtraction,
    /// Calibration warmup / unclassified.
    Sidecar,
}

impl LlmCallType {
    pub fn as_str(self) -> &'static str {
        match self {
            LlmCallType::Layer6Extraction => "layer6_extraction",
            LlmCallType::RuleAuthoring => "rule_authoring",
            LlmCallType::MerchantCleanup => "merchant_cleanup",
            LlmCallType::StatementRowExtraction => "statement_row_extraction",
            LlmCallType::Sidecar => "sidecar",
        }
    }

    fn label(self) -> &'static str {
        match self {
            LlmCallType::Layer6Extraction => "Email → Transaction Extraction",
            LlmCallType::RuleAuthoring => "Rule Authoring (User Correction)",
            LlmCallType::MerchantCleanup => "Merchant / Category Cleanup",
            LlmCallType::StatementRowExtraction => "Statement PDF Row Extraction",
            LlmCallType::Sidecar => "Calibration / Sidecar Warmup",
        }
    }
}

/// Outcome of one sidecar completion attempt.
#[derive(Debug, Clone, Copy)]
pub enum LlmOutcome {
    Accepted,
    RejectedJson,
    RejectedValidation,
    TimedOut,
    InfraFailed,
}

impl LlmOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            LlmOutcome::Accepted => "accepted",
            LlmOutcome::RejectedJson => "rejected_json",
            LlmOutcome::RejectedValidation => "rejected_validation",
            LlmOutcome::TimedOut => "timed_out",
            LlmOutcome::InfraFailed => "infra_failed",
        }
    }

    fn display_label(self) -> &'static str {
        match self {
            LlmOutcome::Accepted => "✓ ACCEPTED",
            LlmOutcome::RejectedJson => "✗ REJECTED  (json unparseable)",
            LlmOutcome::RejectedValidation => "✗ REJECTED  (validation — field not in source)",
            LlmOutcome::TimedOut => "⚠ TIMED OUT  (exceeded calibrated timeout)",
            LlmOutcome::InfraFailed => "✗ INFRA FAILED  (sidecar down / HTTP error)",
        }
    }
}

/// Contextual metadata carried from a call site down to `raw_complete` so
/// every log block is fully attributed without global state.
#[derive(Debug, Clone, Copy)]
pub struct LlmCallContext {
    /// Which subsystem is making this call.
    pub call_type: LlmCallType,
    /// Attempt number within the same logical request (1 = first try,
    /// 2 = self-correction retry, etc.).
    pub attempt: u8,
    /// Total number of attempts expected for this call type (used for "1/2" rendering).
    pub max_attempts: u8,
}

impl LlmCallContext {
    /// Convenience constructor.
    pub fn new(call_type: LlmCallType, attempt: u8) -> Self {
        let max_attempts = match call_type {
            LlmCallType::Layer6Extraction => 2,
            _ => 1,
        };
        Self { call_type, attempt, max_attempts }
    }

    /// Default context for calibration / unclassified callers.
    pub fn unclassified() -> Self {
        Self { call_type: LlmCallType::Sidecar, attempt: 1, max_attempts: 1 }
    }
}

// ─── Box-drawing helpers ───────────────────────────────────────────────────────

fn top_border() -> String {
    format!("┌{}┐", "─".repeat(BOX_WIDTH - 2))
}
fn mid_border() -> String {
    format!("├{}┤", "─".repeat(BOX_WIDTH - 2))
}
fn bot_border() -> String {
    format!("└{}┘", "─".repeat(BOX_WIDTH - 2))
}

/// Render a single box row: `│  <content padded to BOX_WIDTH-4>  │`
fn row(content: &str) -> String {
    // Strip any embedded newlines from structured field content
    let content = content.replace('\n', " ").replace('\r', "");
    let inner_width = BOX_WIDTH - 4; // 2 for "│ " on each side
    if content.len() <= inner_width {
        format!("│  {:<width$}  │", content, width = inner_width)
    } else {
        // Wrap long content across multiple rows
        let mut out = String::new();
        let mut remaining = content.as_str();
        while !remaining.is_empty() {
            let (chunk, rest) = if remaining.len() <= inner_width {
                (remaining, "")
            } else {
                // Try to break on a space
                let cut = remaining[..inner_width]
                    .rfind(' ')
                    .map(|p| p + 1)
                    .unwrap_or(inner_width);
                (&remaining[..cut], remaining[cut..].trim_start())
            };
            let _ = writeln!(out, "│  {:<width$}  │", chunk, width = inner_width);
            remaining = rest;
        }
        out.trim_end_matches('\n').to_string()
    }
}

/// Render a key-value row: `│  Key           : value …  │`
fn kv(key: &str, value: &str) -> String {
    let label = format!("{:<14}: {}", key, value);
    row(&label)
}

/// Render a multi-line body section (prompt or output), wrapping at BOX_WIDTH.
fn body_rows(text: &str, max_chars: usize) -> String {
    let truncated = if text.len() > max_chars {
        let s = &text[..max_chars];
        // Find a safe char boundary
        let safe = s
            .char_indices()
            .rev()
            .find(|(_, c)| *c == ' ' || *c == '\n')
            .map(|(i, _)| i)
            .unwrap_or(max_chars.min(s.len()));
        format!("{} …[{} more chars]", &s[..safe].trim_end(), text.len() - safe)
    } else {
        text.to_string()
    };

    let inner_width = BOX_WIDTH - 4;
    let mut out = String::new();
    for raw_line in truncated.split('\n') {
        if raw_line.is_empty() {
            let _ = writeln!(out, "│  {:<width$}  │", "", width = inner_width);
            continue;
        }
        let mut remaining = raw_line;
        while !remaining.is_empty() {
            let (chunk, rest) = if remaining.len() <= inner_width {
                (remaining, "")
            } else {
                let cut = remaining[..inner_width]
                    .rfind(' ')
                    .map(|p| p + 1)
                    .unwrap_or(inner_width);
                (&remaining[..cut], remaining[cut..].trim_start())
            };
            let _ = writeln!(out, "│  {:<width$}  │", chunk, width = inner_width);
            remaining = rest;
        }
    }
    out.trim_end_matches('\n').to_string()
}

fn now_utc() -> String {
    // Use std time formatted as YYYY-MM-DD HH:MM:SS UTC
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    let days = secs / 86400;
    // Simple Gregorian date from epoch
    let (y, mo, d) = epoch_days_to_ymd(days);
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC", y, mo, d, h, m, s)
}

pub(crate) fn epoch_days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Tomohiko Sakamoto's algorithm adapted for epoch days
    let mut y = 1970u64;
    let mut remaining = days;
    loop {
        let leap = if y % 400 == 0 { 1 } else if y % 100 == 0 { 0 } else if y % 4 == 0 { 1 } else { 0 };
        let days_in_year = 365 + leap;
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        y += 1;
    }
    let leap = if y % 400 == 0 { 1 } else if y % 100 == 0 { 0 } else if y % 4 == 0 { 1 } else { 0 };
    let month_days = [31u64, 28 + leap, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut mo = 1u64;
    for &md in &month_days {
        if remaining < md {
            break;
        }
        remaining -= md;
        mo += 1;
    }
    (y, mo, remaining + 1)
}

/// Write a rich multi-line block directly to a byte sink (the non-blocking writer).
fn write_request_block(
    sink: &mut dyn IoWrite,
    model_id: &str,
    ctx: &LlmCallContext,
    prompt: &str,
) {
    let ts = now_utc();
    let call_type_str = ctx.call_type.as_str();
    let attempt_str = if ctx.max_attempts > 1 {
        format!("attempt {}/{}", ctx.attempt, ctx.max_attempts)
    } else {
        format!("attempt {}", ctx.attempt)
    };
    let header = format!(
        "  ▶ LLM REQUEST  [{}]  {}  {}",
        call_type_str, attempt_str, ts
    );
    let prompt_chars = prompt.len();

    let mut buf = String::with_capacity(512);
    buf.push('\n');
    buf.push_str(&top_border());
    buf.push('\n');
    buf.push_str(&row(&header));
    buf.push('\n');
    buf.push_str(&mid_border());
    buf.push('\n');
    buf.push_str(&kv("Model", model_id));
    buf.push('\n');
    buf.push_str(&kv("Type", ctx.call_type.label()));
    buf.push('\n');
    buf.push_str(&kv("Attempt", &attempt_str));
    buf.push('\n');
    buf.push_str(&kv("Prompt size", &format!("{} chars", fmt_num(prompt_chars))));
    buf.push('\n');
    buf.push_str(&mid_border());
    buf.push('\n');
    buf.push_str(&row("  PROMPT:"));
    buf.push('\n');
    buf.push_str(&body_rows(prompt, PROMPT_PREVIEW_CHARS));
    buf.push('\n');
    buf.push_str(&bot_border());
    buf.push('\n');

    let _ = sink.write_all(buf.as_bytes());
}

fn write_response_block(
    sink: &mut dyn IoWrite,
    model_id: &str,
    ctx: &LlmCallContext,
    duration_ms: u64,
    outcome: LlmOutcome,
    raw_output: Option<&str>,
) {
    let ts = now_utc();
    let call_type_str = ctx.call_type.as_str();
    let attempt_str = if ctx.max_attempts > 1 {
        format!("attempt {}/{}", ctx.attempt, ctx.max_attempts)
    } else {
        format!("attempt {}", ctx.attempt)
    };
    let header = format!(
        "  ◀ LLM RESPONSE [{}]  {}  {}",
        call_type_str, attempt_str, ts
    );
    let output_chars = raw_output.map(|o| o.len()).unwrap_or(0);

    let mut buf = String::with_capacity(512);
    buf.push('\n');
    buf.push_str(&top_border());
    buf.push('\n');
    buf.push_str(&row(&header));
    buf.push('\n');
    buf.push_str(&mid_border());
    buf.push('\n');
    buf.push_str(&kv("Model", model_id));
    buf.push('\n');
    buf.push_str(&kv("Type", ctx.call_type.label()));
    buf.push('\n');
    buf.push_str(&kv("Attempt", &attempt_str));
    buf.push('\n');
    buf.push_str(&kv("Duration", &format!("{} ms", fmt_num(duration_ms as usize))));
    buf.push('\n');
    buf.push_str(&kv("Outcome", outcome.display_label()));
    buf.push('\n');
    buf.push_str(&kv("Output size", &format!("{} chars", fmt_num(output_chars))));
    buf.push('\n');

    if let Some(raw) = raw_output {
        if !raw.trim().is_empty() {
            buf.push_str(&mid_border());
            buf.push('\n');
            buf.push_str(&row("  RAW OUTPUT:"));
            buf.push('\n');
            buf.push_str(&body_rows(raw, OUTPUT_PREVIEW_CHARS));
            buf.push('\n');
        }
    }

    buf.push_str(&bot_border());
    buf.push('\n');

    let _ = sink.write_all(buf.as_bytes());
}

fn fmt_num(n: usize) -> String {
    // Insert thousands separators
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i != 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

// ─── Public API (called from llama_sidecar::raw_complete) ─────────────────────

/// Emitted once per sidecar call, *before* the HTTP request fires.
///
/// Writes a rich bordered block directly to the `llm_calls` writer. Also emits
/// a compact `tracing::info!` event so the event appears in `combined.log`.
pub fn log_llm_request(model_id: &str, ctx: &LlmCallContext, prompt: &str) {
    // Compact structured event → combined.log and backend.log
    let call_type = ctx.call_type.as_str();
    let attempt = ctx.attempt;
    let prompt_chars = prompt.len();
    tracing::info!(
        target: "llm_calls",
        model = model_id,
        call_type,
        attempt,
        prompt_chars,
        "LLM request"
    );

    // Rich block → llm_calls.log via the direct writer
    // We use the global LLM_LOG_WRITER to bypass the tracing event formatter
    // and write structured human-readable blocks directly.
    with_llm_writer(|w| write_request_block(w, model_id, ctx, prompt));
}

/// Emitted once per sidecar call, *after* the HTTP response (or error/timeout).
///
/// Writes a rich bordered block directly to the `llm_calls` writer. Also emits
/// a compact `tracing::info!` event so the event appears in `combined.log`.
pub fn log_llm_response(
    model_id: &str,
    ctx: &LlmCallContext,
    duration_ms: u64,
    outcome: LlmOutcome,
    raw_output: Option<&str>,
) {
    // Compact structured event → combined.log
    let call_type = ctx.call_type.as_str();
    let attempt = ctx.attempt;
    let outcome_str = outcome.as_str();
    let output_chars = raw_output.map(|o| o.len()).unwrap_or(0);
    tracing::info!(
        target: "llm_calls",
        model = model_id,
        call_type,
        attempt,
        duration_ms,
        outcome = outcome_str,
        output_chars,
        "LLM response"
    );

    // Rich block → llm_calls.log
    with_llm_writer(|w| write_response_block(w, model_id, ctx, duration_ms, outcome, raw_output));
}

// ─── Direct writer access ──────────────────────────────────────────────────────
//
// The rich blocks are written directly to the file, bypassing the tracing event
// formatter (which would mangle the box-drawing characters into a single line).
// We hold a global `Mutex<Option<File>>` that `CategorizedLogWriters::init`
// seeds once. Callers that fire before init (tests) silently drop the write.

use std::sync::{Mutex, OnceLock};
use std::fs::OpenOptions;

static LLM_LOG_PATH: OnceLock<std::path::PathBuf> = OnceLock::new();
static LLM_LOG_WRITER: OnceLock<Mutex<std::fs::File>> = OnceLock::new();

/// Called once by `CategorizedLogWriters::init` with the path of the
/// current day's `llm_calls.log.*` file so rich blocks can be written directly.
pub fn init_direct_writer(path: std::path::PathBuf) {
    let _ = LLM_LOG_PATH.set(path.clone());
    if let Ok(file) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = LLM_LOG_WRITER.set(Mutex::new(file));
    }
}

fn with_llm_writer<F: FnOnce(&mut dyn IoWrite)>(f: F) {
    if let Some(mutex) = LLM_LOG_WRITER.get() {
        if let Ok(mut guard) = mutex.lock() {
            f(&mut *guard);
        }
    }
    // If the writer isn't initialised (tests, early startup) — silently drop.
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_block_renders_without_panicking() {
        let ctx = LlmCallContext::new(LlmCallType::Layer6Extraction, 1);
        let mut buf: Vec<u8> = Vec::new();
        write_request_block(&mut buf, "gemma4_e4b", &ctx, "Extract the following fields from a bank transaction alert…");
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("▶ LLM REQUEST"));
        assert!(s.contains("layer6_extraction"));
        assert!(s.contains("gemma4_e4b"));
        assert!(s.contains("PROMPT:"));
        assert!(s.contains("Extract the following fields"));
        assert!(s.contains("┌") && s.contains("┘"));
    }

    #[test]
    fn response_block_renders_accepted() {
        let ctx = LlmCallContext::new(LlmCallType::Layer6Extraction, 1);
        let mut buf: Vec<u8> = Vec::new();
        write_response_block(
            &mut buf,
            "gemma4_e4b",
            &ctx,
            4764,
            LlmOutcome::Accepted,
            Some(r#"{"amount": 1299.00, "currency": "INR"}"#),
        );
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("◀ LLM RESPONSE"));
        assert!(s.contains("✓ ACCEPTED"));
        assert!(s.contains("4,764 ms"));
        assert!(s.contains("RAW OUTPUT:"));
        assert!(s.contains("1299.00"));
    }

    #[test]
    fn response_block_renders_timeout_without_output() {
        let ctx = LlmCallContext::new(LlmCallType::Layer6Extraction, 2);
        let mut buf: Vec<u8> = Vec::new();
        write_response_block(&mut buf, "gemma4_e4b", &ctx, 10001, LlmOutcome::TimedOut, None);
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("⚠ TIMED OUT"));
        assert!(!s.contains("RAW OUTPUT:"));
    }

    #[test]
    fn response_block_renders_infra_failed() {
        let ctx = LlmCallContext::unclassified();
        let mut buf: Vec<u8> = Vec::new();
        write_response_block(&mut buf, "gemma4_e4b", &ctx, 11, LlmOutcome::InfraFailed, None);
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("✗ INFRA FAILED"));
    }

    #[test]
    fn outcome_as_str_roundtrips() {
        assert_eq!(LlmOutcome::Accepted.as_str(), "accepted");
        assert_eq!(LlmOutcome::RejectedJson.as_str(), "rejected_json");
        assert_eq!(LlmOutcome::RejectedValidation.as_str(), "rejected_validation");
        assert_eq!(LlmOutcome::TimedOut.as_str(), "timed_out");
        assert_eq!(LlmOutcome::InfraFailed.as_str(), "infra_failed");
    }

    #[test]
    fn call_type_as_str_roundtrips() {
        assert_eq!(LlmCallType::Layer6Extraction.as_str(), "layer6_extraction");
        assert_eq!(LlmCallType::RuleAuthoring.as_str(), "rule_authoring");
        assert_eq!(LlmCallType::MerchantCleanup.as_str(), "merchant_cleanup");
        assert_eq!(LlmCallType::StatementRowExtraction.as_str(), "statement_row_extraction");
        assert_eq!(LlmCallType::Sidecar.as_str(), "sidecar");
    }

    #[test]
    fn fmt_num_inserts_thousands_separators() {
        assert_eq!(fmt_num(0), "0");
        assert_eq!(fmt_num(999), "999");
        assert_eq!(fmt_num(1000), "1,000");
        assert_eq!(fmt_num(2805), "2,805");
        assert_eq!(fmt_num(10001), "10,001");
        assert_eq!(fmt_num(1_000_000), "1,000,000");
    }

    #[test]
    fn log_llm_request_does_not_panic_without_writer() {
        let ctx = LlmCallContext::new(LlmCallType::Layer6Extraction, 1);
        log_llm_request("gemma4_e4b", &ctx, "test prompt");
    }

    #[test]
    fn log_llm_response_does_not_panic_without_writer() {
        let ctx = LlmCallContext::new(LlmCallType::Layer6Extraction, 1);
        log_llm_response("gemma4_e4b", &ctx, 4000, LlmOutcome::Accepted, Some("{}"));
    }
}
