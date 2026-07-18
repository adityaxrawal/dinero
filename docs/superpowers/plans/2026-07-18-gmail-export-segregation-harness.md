# Gmail Export Segregation Harness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replay the full Gmail export (`gmail_export/emails.jsonl`, 5,873 records) through Dinero's real gate1/gate2/gate3 classification logic via a single Rust batch binary, sort each email into `gmail_export/segregated_emails/{transaction,non_transaction,statement}/<id>.json`, and provide a CLI to hand-label mispredictions for later tuning.

**Architecture:** A new Rust binary `src-tauri/src/bin/email_segregate.rs` links `dinero_app_lib`, copies the real encrypted `finance.db` to a scratch path, opens it via `db::init_db`, and streams `emails.jsonl` directly (no IPC, no driver process) through the real `SenderValidator` / `ContentClassifier` / `run_extraction_ladder` gates, writing one output JSON file per email as it goes. Since `llm_eligible` is always `false` the workload is pure regex + local SQLite reads, so records are classified concurrently via `futures_util::stream::buffer_unordered` over the async gate calls — `deadpool_sqlite::Pool` is cheaply `Clone`, so the same pool is shared across concurrent tasks. Resumability comes from scanning the output directory for existing `<id>.json` files before starting and skipping those ids. A separate Python script (`gmail_export/review.py`) lets a human mark ground truth against the algorithm's prediction and print a confusion matrix — kept in Python since it's a small, rerun-often CLI with no performance surface, not part of the batch job.

**Tech Stack:** Rust (existing `dinero_app_lib` crate, tokio, serde_json, futures-util — all already dependencies, no `Cargo.toml` change needed), Python 3.14 stdlib only for `review.py` (no new pip dependencies).

## Post-execution notes (deviations found during implementation)

- **Build mode matters for Keychain access.** `email_segregate` must be built with plain `cargo build` (debug), not `--release`. The real app on this machine runs via `cargo tauri dev` (debug), which uses `get_or_create_base_key`'s `#[cfg(debug_assertions)]` path — a plaintext key file at `$TMPDIR/dinero_dev_base_key.txt` — not Keychain. A release build hits the Keychain-backed path instead, and since this ad-hoc binary isn't signed with the same identity as `Dinero.app`, `Entry::get_password()` returns `NoEntry` and silently generates an unrelated random key, producing `DbInitError::KeyMismatch` against the real data (confirmed via `security find-generic-password` also returning "item not found" from an interactive terminal — it was never in Keychain to begin with). The `--base-key-file`/`open_pool` escape hatch built into Task 2 for the Keychain scenario is unused on this machine but kept as a fallback for a future signed/release setup.
- **`review.py` moved to `scripts/review_gmail_export.py`.** The plan's original `gmail_export/review.py` path is unreachable by git — `.gitignore`'s `/gmail_export` line ignores the whole directory, and a directory-level ignore blocks git from even descending in to honor a narrower `!gmail_export/review.py` negation. Source files that need to be tracked can't live under `gmail_export/`; moved to this repo's existing `scripts/` convention instead. Data outputs (`segregated_emails/`, `review_log.jsonl`, `.scratch_db/`) correctly stay under `gmail_export/` and gitignored.
- **`EmailRecord` needed null-safe string deserialization.** `#[serde(default)]` only covers a *missing* key — several export records have `"from": null` explicitly present, which still fails to deserialize into `String`. Added a `null_to_default` deserializer (`Option::<String>::deserialize(..).unwrap_or_default()`) applied to `from`/`subject`/`body_text`/`body_html`/attachment `path`.

## Global Constraints

- Real database file is `finance.db` (not `data.db`) at `~/Library/Application Support/com.dinero.app/finance.db`, in SQLCipher WAL mode — the live data lives partly in the sibling `finance.db-wal` file, so all three of `finance.db` / `finance.db-wal` / `finance.db-shm` must be copied together. (`com.adityarawal.dinero-app/data.db` is a stale directory from an old bundle identifier — do not use it.)
- Never open `db::init_db` against the real path — always against a scratch copy under `gmail_export/.scratch_db/` (already covered by the existing `/gmail_export` line in `.gitignore`, so no new gitignore entry is needed).
- `llm_eligible` is always `false` when calling `run_extraction_ladder` — this deterministically skips Layer 6 (LLM), confirmed by reading `run_extraction_ladder`'s own early-return when `!llm_eligible`.
- `gmail_export/emails.jsonl` has 5,873 records; 31.9% have an empty `body_text` and must fall back to a sanitized `body_html` — mirror production's real fallback (`mime_sanitization::extract_body_and_attachments`: use `body_text` if non-empty, else `sanitize_html(body_html)`, then always run the result through `sanitize_plain_text`). Both `sanitize_html` and `sanitize_plain_text` are `pub fn` in `dinero_app_lib::ingestion::mime_sanitization` — call them directly, do not reimplement.
- `parse_from_header`, `evaluate_mandatory_field_gate`, `gate3_failure_reason`, and `internal_date_fallback` in `message_processor.rs` are private / `pub(crate)` — invisible across the bin/lib crate boundary even within the same Cargo package. All four must be reimplemented verbatim inside the new binary (confirmed by reading each definition in `src-tauri/src/ingestion/message_processor.rs`).
- `ExtractionResult` (in `src-tauri/src/extraction/ladder.rs`) derives `Debug, Clone, PartialEq, Default` but **not** `Serialize` — do not add a derive to production code; build the JSON manually field-by-field.
- `deadpool_sqlite::Pool` (deadpool 0.13, confirmed by reading `deadpool-0.13.0/src/managed/pool.rs`) implements `Clone` — cloning it is cheap (Arc-based), safe to share across concurrent `tokio` tasks. `SenderValidator` does not derive `Clone` (it re-parses an embedded JSON registry in `::new()`, so it must only be constructed once) — share it via `Arc<SenderValidator>` instead.
- Cargo auto-discovers `src-tauri/src/bin/*.rs` as binaries (confirmed: `pdf_sidecar.rs` has no corresponding `[[bin]]` entry in `Cargo.toml`) — no `Cargo.toml` edit needed for the new binary.

---

## File Structure

- Create: `src-tauri/src/bin/email_segregate.rs` — the batch binary (DB copy, resumable streaming, concurrent gate1–3 classification, output-file writing, pure helper reimplementations + unit tests).
- Create: `gmail_export/review.py` — Python review CLI: `mark` and `report` subcommands over `gmail_export/review_log.jsonl`.
- Output (gitignored via existing `/gmail_export` rule): `gmail_export/segregated_emails/{transaction,non_transaction,statement}/<id>.json`, `gmail_export/review_log.jsonl`, `gmail_export/.scratch_db/finance.db{,-wal,-shm}`.

---

### Task 1: Pure-logic core (parse/gate helpers + body fallback)

**Files:**
- Create: `src-tauri/src/bin/email_segregate.rs` (this task only adds the pure functions + their tests; `main()` comes in Task 2)

**Interfaces:**
- Produces: `fn parse_from_header(from: &str) -> (String, Option<String>)`, `fn evaluate_mandatory_field_gate(obs: &dinero_app_lib::extraction::ladder::ExtractionResult) -> bool`, `fn gate3_failure_reason(obs: &dinero_app_lib::extraction::ladder::ExtractionResult) -> &'static str`, `fn internal_date_fallback(internal_date_ms: Option<i64>) -> Option<i64>`, `fn effective_body(body_text: &str, body_html: &str) -> String` — all consumed by Task 2's `classify_one`.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/bin/email_segregate.rs` with just the test module and empty `fn main() {}` so it compiles as a binary target:

```rust
fn main() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_from_header_extracts_display_name_and_email() {
        assert_eq!(
            parse_from_header("YES BANK Alerts <alerts@yes.bank.in>"),
            ("alerts@yes.bank.in".to_string(), Some("YES BANK Alerts".to_string()))
        );
    }

    #[test]
    fn parse_from_header_strips_quotes_from_display_name() {
        assert_eq!(
            parse_from_header("\"Bank, Inc.\" <ops@bank.com>"),
            ("ops@bank.com".to_string(), Some("Bank, Inc.".to_string()))
        );
    }

    #[test]
    fn parse_from_header_bare_address_has_no_display_name() {
        assert_eq!(
            parse_from_header("noreply@bank.com"),
            ("noreply@bank.com".to_string(), None)
        );
    }

    #[test]
    fn mandatory_field_gate_passes_on_amount_and_entity() {
        let obs = dinero_app_lib::extraction::ladder::ExtractionResult {
            amount_minor: Some(100),
            merchant_raw: Some("Store".to_string()),
            ..Default::default()
        };
        assert!(evaluate_mandatory_field_gate(&obs));
    }

    #[test]
    fn mandatory_field_gate_passes_on_balance_alone() {
        let obs = dinero_app_lib::extraction::ladder::ExtractionResult {
            balance_after: Some(500),
            ..Default::default()
        };
        assert!(evaluate_mandatory_field_gate(&obs));
    }

    #[test]
    fn mandatory_field_gate_fails_on_amount_without_entity_or_balance() {
        let obs = dinero_app_lib::extraction::ladder::ExtractionResult {
            amount_minor: Some(100),
            ..Default::default()
        };
        assert!(!evaluate_mandatory_field_gate(&obs));
    }

    #[test]
    fn gate3_failure_reason_reports_missing_amount() {
        let obs = dinero_app_lib::extraction::ladder::ExtractionResult::default();
        assert_eq!(gate3_failure_reason(&obs), "gate3_failed:missing_amount");
    }

    #[test]
    fn gate3_failure_reason_reports_missing_counterparty() {
        let obs = dinero_app_lib::extraction::ladder::ExtractionResult {
            amount_minor: Some(100),
            ..Default::default()
        };
        assert_eq!(gate3_failure_reason(&obs), "gate3_failed:missing_counterparty");
    }

    #[test]
    fn internal_date_fallback_converts_ms_to_seconds() {
        assert_eq!(internal_date_fallback(Some(1783697136000)), Some(1783697136));
    }

    #[test]
    fn internal_date_fallback_none_stays_none() {
        assert_eq!(internal_date_fallback(None), None);
    }

    #[test]
    fn effective_body_prefers_nonempty_body_text() {
        assert_eq!(effective_body("Hello World", "<p>ignored</p>"), "Hello World");
    }

    #[test]
    fn effective_body_falls_back_to_sanitized_html_when_text_empty() {
        let body = effective_body("", "<p>Hi there</p>");
        assert!(body.contains("Hi there"));
        assert!(!body.contains('<'));
    }

    #[test]
    fn effective_body_both_empty_yields_empty_string() {
        assert_eq!(effective_body("", ""), "");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test --bin email_segregate`
Expected: FAIL — `parse_from_header`, `evaluate_mandatory_field_gate`, `gate3_failure_reason`, `internal_date_fallback`, `effective_body` not found in scope.

- [ ] **Step 3: Implement the pure functions**

Replace `fn main() {}` at the top of `src-tauri/src/bin/email_segregate.rs` with:

```rust
use dinero_app_lib::extraction::ladder::ExtractionResult;
use dinero_app_lib::ingestion::mime_sanitization::{sanitize_html, sanitize_plain_text};

fn main() {}

/// Mirrors the private `MessageProcessor::parse_from_header` in
/// `src-tauri/src/ingestion/message_processor.rs` — that fn is not `pub`,
/// so it is unreachable from this bin crate; reimplemented verbatim.
fn parse_from_header(from: &str) -> (String, Option<String>) {
    if let (Some(start), Some(end)) = (from.find('<'), from.rfind('>')) {
        if start < end {
            let email = from[start + 1..end].trim().to_string();
            let name_part = from[..start].trim();
            let display_name = if name_part.is_empty() {
                None
            } else {
                Some(
                    name_part
                        .trim_matches(|c| c == '"' || c == ' ' || c == '\'')
                        .to_string(),
                )
            };
            return (email, display_name);
        }
    }
    (from.trim().to_string(), None)
}

/// Mirrors `MessageProcessor::evaluate_mandatory_field_gate`, which is
/// `pub(crate)` and therefore invisible across the bin/lib crate boundary.
fn evaluate_mandatory_field_gate(obs: &ExtractionResult) -> bool {
    let has_amount = obs.amount_minor.is_some();
    let has_entity = obs.merchant_raw.is_some();
    let has_balance = obs.balance_after.is_some();
    (has_amount && has_entity) || has_balance
}

/// Mirrors `MessageProcessor::gate3_failure_reason` (also `pub(crate)`).
fn gate3_failure_reason(obs: &ExtractionResult) -> &'static str {
    let has_amount = obs.amount_minor.is_some();
    let has_entity = obs.merchant_raw.is_some();
    match (has_amount, has_entity) {
        (false, _) => "gate3_failed:missing_amount",
        (true, false) => "gate3_failed:missing_counterparty",
        (true, true) => "gate3_failed",
    }
}

/// Mirrors `MessageProcessor::internal_date_fallback`, adapted for the
/// export's `internalDate` already being a JSON integer (milliseconds)
/// rather than the Gmail API's numeric-string.
fn internal_date_fallback(internal_date_ms: Option<i64>) -> Option<i64> {
    internal_date_ms.map(|ms| ms / 1000)
}

/// Mirrors `mime_sanitization::extract_body_and_attachments`'s fallback
/// logic (use `body_text` if present, else sanitize `body_html`), needed
/// here because the export's `body_text` is empty on ~32% of records.
fn effective_body(body_text: &str, body_html: &str) -> String {
    let raw = if !body_text.trim().is_empty() {
        body_text.to_string()
    } else if !body_html.trim().is_empty() {
        sanitize_html(body_html)
    } else {
        String::new()
    };
    sanitize_plain_text(&raw)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test --bin email_segregate`
Expected: PASS — all 12 tests green.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/bin/email_segregate.rs
git commit -m "feat(gmail-export): add pure gate-helper reimplementations for email segregate binary"
```

---

### Task 2: Batch binary — DB copy, resumable concurrent streaming, output writing

**Files:**
- Modify: `src-tauri/src/bin/email_segregate.rs`

**Interfaces:**
- Consumes: Task 1's `parse_from_header`, `evaluate_mandatory_field_gate`, `gate3_failure_reason`, `internal_date_fallback`, `effective_body`; `dinero_app_lib::db::init_db(db_path: PathBuf) -> Result<Pool, DbInitError>`; `dinero_app_lib::ingestion::verified_senders::{SenderValidator, SenderVerificationResult}`; `dinero_app_lib::ingestion::content_classifier::{ContentClassifier, ContentClass}`; `dinero_app_lib::extraction::ladder::{run_extraction_ladder, ExtractionResult}`.
- Produces: the `email_segregate` binary itself, invoked as `email_segregate [--flags]`, reading `emails.jsonl` and writing `gmail_export/segregated_emails/{label}/<id>.json` files directly — no stdin/stdout protocol.

- [ ] **Step 1: Replace `fn main() {}` with DB copy, CLI args, and the concurrent pipeline**

Add these imports at the top of `src-tauri/src/bin/email_segregate.rs` (alongside the Task 1 imports):

```rust
use dinero_app_lib::db;
use dinero_app_lib::ingestion::content_classifier::{ContentClass, ContentClassifier};
use dinero_app_lib::ingestion::verified_senders::{SenderValidator, SenderVerificationResult};
use futures_util::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
```

Replace `fn main() {}` with:

```rust
const LABELS: [&str; 3] = ["transaction", "non_transaction", "statement"];
const DB_FILES: [&str; 3] = ["finance.db", "finance.db-wal", "finance.db-shm"];

#[derive(Debug, Deserialize)]
struct EmailAttachment {
    #[serde(default)]
    path: String,
}

#[derive(Debug, Deserialize)]
struct EmailRecord {
    #[serde(default)]
    from: String,
    #[serde(default)]
    subject: String,
    #[serde(default)]
    body_text: String,
    #[serde(default)]
    body_html: String,
    #[serde(rename = "internalDate", default)]
    internal_date: Option<i64>,
    #[serde(default)]
    attachments: Vec<EmailAttachment>,
}

#[derive(Debug, Serialize)]
struct ClassifyResult {
    predicted_label: String,
    gate1_result: String,
    gate1_bank_name: Option<String>,
    gate2_result: Option<String>,
    gate3_extraction: Option<serde_json::Value>,
    rejection_reason: Option<String>,
    attachment_paths: Vec<String>,
    sidecar_version: String,
}

struct Args {
    emails_jsonl: PathBuf,
    output_dir: PathBuf,
    db_source: PathBuf,
    scratch_db_dir: PathBuf,
    progress_every: usize,
    reuse_db_copy: bool,
    concurrency: usize,
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri always has a parent directory")
        .to_path_buf()
}

fn home_dir() -> PathBuf {
    PathBuf::from(std::env::var("HOME").expect("HOME env var not set"))
}

fn parse_args() -> Args {
    let root = repo_root();
    let mut args = Args {
        emails_jsonl: root.join("gmail_export/emails.jsonl"),
        output_dir: root.join("gmail_export/segregated_emails"),
        db_source: home_dir().join("Library/Application Support/com.dinero.app"),
        scratch_db_dir: root.join("gmail_export/.scratch_db"),
        progress_every: 200,
        reuse_db_copy: false,
        concurrency: 8,
    };

    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut next = || it.next().unwrap_or_else(|| panic!("{flag} needs a value"));
        match flag.as_str() {
            "--emails-jsonl" => args.emails_jsonl = PathBuf::from(next()),
            "--output-dir" => args.output_dir = PathBuf::from(next()),
            "--db-source" => args.db_source = PathBuf::from(next()),
            "--scratch-db-dir" => args.scratch_db_dir = PathBuf::from(next()),
            "--progress-every" => args.progress_every = next().parse().expect("--progress-every must be a number"),
            "--concurrency" => args.concurrency = next().parse().expect("--concurrency must be a number"),
            "--reuse-db-copy" => args.reuse_db_copy = true,
            other => panic!("unknown flag: {other}"),
        }
    }
    args
}

fn copy_db(source_dir: &Path, scratch_dir: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(scratch_dir)?;
    eprintln!(
        "Copying database from {} -> {}\nMake sure the Dinero app is fully quit so the WAL file is consistent.",
        source_dir.display(),
        scratch_dir.display()
    );
    for fname in DB_FILES {
        let src = source_dir.join(fname);
        if src.exists() {
            std::fs::copy(&src, scratch_dir.join(fname))?;
        }
    }
    Ok(scratch_dir.join("finance.db"))
}

fn already_processed_ids(output_dir: &Path) -> HashSet<String> {
    let mut ids = HashSet::new();
    for label in LABELS {
        let dir = output_dir.join(label);
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                if let Some(stem) = entry.path().file_stem().and_then(|s| s.to_str()) {
                    ids.insert(stem.to_string());
                }
            }
        }
    }
    ids
}

fn git_short_sha() -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn gate1_variant_name(result: &SenderVerificationResult) -> &'static str {
    match result {
        SenderVerificationResult::VerifiedTransactionCandidate(_) => "VerifiedTransactionCandidate",
        SenderVerificationResult::VerifiedStatementCandidate(_) => "VerifiedStatementCandidate",
        SenderVerificationResult::VerifiedNoise => "VerifiedNoise",
        SenderVerificationResult::UnverifiedReject(_) => "UnverifiedReject",
        SenderVerificationResult::SpoofReject(_) => "SpoofReject",
    }
}

fn extraction_to_json(obs: &ExtractionResult) -> serde_json::Value {
    serde_json::json!({
        "amount_minor": obs.amount_minor,
        "currency": obs.currency,
        "direction": obs.direction,
        "event_time": obs.event_time,
        "merchant_raw": obs.merchant_raw,
        "reference_id": obs.reference_id,
        "balance_after": obs.balance_after,
        "original_amount_minor": obs.original_amount_minor,
        "original_currency": obs.original_currency,
        "instrument_type": obs.instrument_type,
        "issuer_name": obs.issuer_name,
        "masked_identifier": obs.masked_identifier,
        "network": obs.network,
        "upi_vpa": obs.upi_vpa,
        "extraction_method": obs.extraction_method,
        "confidence_score": obs.confidence_score,
        "parser_version": obs.parser_version,
        "emi_total_installments": obs.emi_total_installments,
        "emi_installment_number": obs.emi_installment_number,
        "emi_original_amount_minor": obs.emi_original_amount_minor,
        "exchange_rate": obs.exchange_rate,
    })
}

async fn classify_one(
    pool: &deadpool_sqlite::Pool,
    validator: &SenderValidator,
    record: &EmailRecord,
    sidecar_version: &str,
) -> ClassifyResult {
    let attachment_paths: Vec<String> = record.attachments.iter().map(|a| a.path.clone()).collect();
    let respond = |predicted_label: &str,
                   gate1_result: &str,
                   gate1_bank_name: Option<String>,
                   gate2_result: Option<String>,
                   gate3_extraction: Option<serde_json::Value>,
                   rejection_reason: Option<String>| ClassifyResult {
        predicted_label: predicted_label.to_string(),
        gate1_result: gate1_result.to_string(),
        gate1_bank_name,
        gate2_result,
        gate3_extraction,
        rejection_reason,
        attachment_paths: attachment_paths.clone(),
        sidecar_version: sidecar_version.to_string(),
    };

    // Gate 1: sender verification.
    let (email, display_name) = parse_from_header(&record.from);
    let gate1 = validator.verify_sender(&email, display_name.as_deref());
    let gate1_result = gate1_variant_name(&gate1);

    let bank_name = match &gate1 {
        SenderVerificationResult::VerifiedTransactionCandidate(b)
        | SenderVerificationResult::VerifiedStatementCandidate(b) => b.clone(),
        SenderVerificationResult::VerifiedNoise
        | SenderVerificationResult::UnverifiedReject(_)
        | SenderVerificationResult::SpoofReject(_) => {
            let reason = format!("gate1_{gate1_result}");
            return respond("non_transaction", gate1_result, None, None, None, Some(reason));
        }
    };

    // Gate 2: content classification.
    let body = effective_body(&record.body_text, &record.body_html);
    let content_class = ContentClassifier::classify(&record.subject, &body);
    let gate2_result = format!("{content_class:?}");

    match content_class {
        ContentClass::StatementEmail => respond(
            "statement",
            gate1_result,
            Some(bank_name),
            Some(gate2_result),
            None,
            None,
        ),
        ContentClass::TransactionAlert | ContentClass::BalanceUpdate => {
            // Gate 3: extraction ladder + mandatory field gate.
            let internal_date_seconds = internal_date_fallback(record.internal_date);
            let extraction = run_extraction_ladder(
                pool,
                &bank_name,
                &body,
                None,
                false,
                internal_date_seconds,
            )
            .await
            .unwrap_or(None);

            match extraction {
                Some(obs) if evaluate_mandatory_field_gate(&obs) => respond(
                    "transaction",
                    gate1_result,
                    Some(bank_name),
                    Some(gate2_result),
                    Some(extraction_to_json(&obs)),
                    None,
                ),
                Some(obs) => {
                    let reason = gate3_failure_reason(&obs).to_string();
                    respond(
                        "non_transaction",
                        gate1_result,
                        Some(bank_name),
                        Some(gate2_result),
                        Some(extraction_to_json(&obs)),
                        Some(reason),
                    )
                }
                None => respond(
                    "non_transaction",
                    gate1_result,
                    Some(bank_name),
                    Some(gate2_result),
                    None,
                    Some("extraction_failed".to_string()),
                ),
            }
        }
        other => {
            let reason = format!("gate2_reject_{other:?}");
            respond(
                "non_transaction",
                gate1_result,
                Some(bank_name),
                Some(gate2_result),
                None,
                Some(reason),
            )
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = parse_args();

    let scratch_db_path = args.scratch_db_dir.join("finance.db");
    let db_path = if args.reuse_db_copy && scratch_db_path.exists() {
        eprintln!("Reusing existing scratch DB at {}", scratch_db_path.display());
        scratch_db_path
    } else {
        copy_db(&args.db_source, &args.scratch_db_dir)?
    };

    for label in LABELS {
        std::fs::create_dir_all(args.output_dir.join(label))?;
    }

    let skip_ids = already_processed_ids(&args.output_dir);
    eprintln!("{} ids already processed — will be skipped.", skip_ids.len());

    let pool = db::init_db(db_path).await.expect("failed to open scratch database copy");
    let validator = Arc::new(SenderValidator::new());
    let sidecar_version = Arc::new(git_short_sha());

    let file = std::fs::File::open(&args.emails_jsonl)?;
    let reader = std::io::BufReader::new(file);

    let counts: Arc<Mutex<HashMap<String, usize>>> = Arc::new(Mutex::new(HashMap::new()));
    let processed = Arc::new(AtomicUsize::new(0));
    let start = std::time::Instant::now();
    let output_dir = args.output_dir.clone();
    let progress_every = args.progress_every;

    let pending = reader.lines().filter_map(move |line| {
        let line = line.expect("failed to read line from emails.jsonl");
        if line.trim().is_empty() {
            return None;
        }
        let raw: serde_json::Value =
            serde_json::from_str(&line).expect("malformed JSON line in emails.jsonl");
        let id = raw.get("id")?.as_str()?.to_string();
        if skip_ids.contains(&id) {
            return None;
        }
        let record: EmailRecord =
            serde_json::from_value(raw.clone()).expect("EmailRecord fields missing/malformed");
        Some((id, raw, record))
    });

    stream::iter(pending)
        .map(|(id, raw, record)| {
            let pool = pool.clone();
            let validator = validator.clone();
            let sidecar_version = sidecar_version.clone();
            let counts = counts.clone();
            let processed = processed.clone();
            let output_dir = output_dir.clone();
            async move {
                let result = classify_one(&pool, &validator, &record, &sidecar_version).await;
                let mut pipeline_json =
                    serde_json::to_value(&result).expect("ClassifyResult always serializes");
                pipeline_json["processed_at"] = serde_json::json!(chrono::Utc::now().to_rfc3339());

                let out = serde_json::json!({ "raw": raw, "pipeline": pipeline_json });
                let out_path = output_dir.join(&result.predicted_label).join(format!("{id}.json"));
                std::fs::write(&out_path, serde_json::to_string_pretty(&out).unwrap())
                    .unwrap_or_else(|e| panic!("failed to write {}: {e}", out_path.display()));

                {
                    let mut c = counts.lock().unwrap();
                    *c.entry(result.predicted_label.clone()).or_insert(0) += 1;
                }
                let n = processed.fetch_add(1, Ordering::Relaxed) + 1;
                if n % progress_every == 0 {
                    let c = counts.lock().unwrap();
                    eprintln!("[{n}] labels={c:?} elapsed={:.1}s", start.elapsed().as_secs_f64());
                }
            }
        })
        .buffer_unordered(args.concurrency)
        .collect::<Vec<()>>()
        .await;

    let final_counts = counts.lock().unwrap();
    eprintln!(
        "Done. processed={} labels={:?} elapsed={:.1}s",
        processed.load(Ordering::Relaxed),
        *final_counts,
        start.elapsed().as_secs_f64()
    );

    Ok(())
}
```

- [ ] **Step 2: Build it**

Run: `cd src-tauri && cargo build --release --bin email_segregate`
Expected: builds cleanly (warnings OK, no errors).

- [ ] **Step 3: Manual smoke test on a 50-record subset**

Quit the Dinero app first (so the WAL file is in a consistent state), then:

```bash
cd "/Users/adityarawal/A-B/Projects/Codes/Finance Tools/Dinero/dinero-app"
head -50 gmail_export/emails.jsonl > /tmp/emails_sample.jsonl
src-tauri/target/release/email_segregate --emails-jsonl /tmp/emails_sample.jsonl --progress-every 10
```

Expected: a macOS Keychain access prompt appears once (approve it), progress lines every 10 records to stderr, a final `Done. processed=50 labels={...}` line, and up to 50 new files distributed across `gmail_export/segregated_emails/{transaction,non_transaction,statement}/`. Spot-check one file: it must have `raw` (the original record) and `pipeline` (with `predicted_label`, `gate1_result`, `gate2_result`, `sidecar_version` set to a short git hash, and `processed_at`). The first sample record (a YES BANK transaction alert with empty `body_text`) should come back with `gate2_result` set to `"TransactionAlert"` or similar, proving the HTML fallback path works.

- [ ] **Step 4: Verify resumability**

Run the exact same command again:

```bash
src-tauri/target/release/email_segregate --emails-jsonl /tmp/emails_sample.jsonl --progress-every 10 --reuse-db-copy
```

Expected: stderr prints `50 ids already processed — will be skipped.` and `Done. processed=0 labels={}` — no new files written, no gate work re-run for already-labeled ids.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/bin/email_segregate.rs
git commit -m "feat(gmail-export): add concurrent DB-copy-and-classify batch binary"
```

---

### Task 3: Python review CLI (`review.py`)

**Files:**
- Create: `gmail_export/review.py`

**Interfaces:**
- Consumes: `gmail_export/segregated_emails/{transaction,non_transaction,statement}/<id>.json` (Task 2's output).
- Produces: `gmail_export/review_log.jsonl`, appended one line per `mark` call: `{"email_id", "predicted_label", "correct_label", "note", "marked_at"}`.

- [ ] **Step 1: Write `gmail_export/review.py`**

```python
#!/usr/bin/env python3
"""Hand-label ground truth against the classify binary's predictions.

Usage:
    review.py mark <email_id> --correct-label {transaction,non_transaction,statement} [--note "..."]
    review.py report
"""
import argparse
import json
import sys
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
OUTPUT_DIR = REPO_ROOT / "gmail_export" / "segregated_emails"
REVIEW_LOG = REPO_ROOT / "gmail_export" / "review_log.jsonl"
LABELS = ["transaction", "non_transaction", "statement"]


def find_segregated_record(email_id: str) -> tuple[str, dict] | tuple[None, None]:
    for label in LABELS:
        path = OUTPUT_DIR / label / f"{email_id}.json"
        if path.exists():
            return label, json.loads(path.read_text())
    return None, None


def cmd_mark(args: argparse.Namespace) -> None:
    label, record = find_segregated_record(args.email_id)
    if record is None:
        sys.exit(
            f"email_id {args.email_id!r} not found in any of "
            f"{[str(OUTPUT_DIR / l) for l in LABELS]}"
        )
    entry = {
        "email_id": args.email_id,
        "predicted_label": record["pipeline"]["predicted_label"],
        "correct_label": args.correct_label,
        "note": args.note,
        "marked_at": datetime.now(timezone.utc).isoformat(),
    }
    with REVIEW_LOG.open("a", encoding="utf-8") as f:
        f.write(json.dumps(entry, ensure_ascii=False) + "\n")
    print(f"Marked {args.email_id}: predicted={entry['predicted_label']} correct={args.correct_label}")


def cmd_report(_args: argparse.Namespace) -> None:
    if not REVIEW_LOG.exists():
        print("No review_log.jsonl yet — run `review.py mark` first.")
        return

    entries = [
        json.loads(line)
        for line in REVIEW_LOG.read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]

    matrix: dict[str, dict[str, int]] = defaultdict(lambda: defaultdict(int))
    mismatches_by_reason: dict[str, list[dict]] = defaultdict(list)

    for e in entries:
        matrix[e["predicted_label"]][e["correct_label"]] += 1
        if e["predicted_label"] != e["correct_label"]:
            _, record = find_segregated_record(e["email_id"])
            pipeline = record["pipeline"] if record else {}
            reason = pipeline.get("rejection_reason") or pipeline.get("gate2_result") or "unknown"
            mismatches_by_reason[reason].append(e)

    print("Confusion matrix (rows = predicted, columns = correct):")
    all_labels = sorted({*matrix.keys(), *(k for v in matrix.values() for k in v)})
    header = "predicted \\ correct".ljust(20) + "".join(l.ljust(18) for l in all_labels)
    print(header)
    for predicted in all_labels:
        row = predicted.ljust(20)
        for correct in all_labels:
            row += str(matrix[predicted][correct]).ljust(18)
        print(row)

    print(f"\nTotal marked: {len(entries)}")
    total_mismatches = sum(len(v) for v in mismatches_by_reason.values())
    print(f"Total mismatches: {total_mismatches}\n")

    if mismatches_by_reason:
        print("Mismatches grouped by rejection_reason/gate2_result:")
        for reason, items in sorted(mismatches_by_reason.items(), key=lambda kv: -len(kv[1])):
            print(f"  {reason}: {len(items)}")
            for item in items:
                print(
                    f"    {item['email_id']}: predicted={item['predicted_label']} "
                    f"correct={item['correct_label']} note={item.get('note')!r}"
                )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)

    mark_parser = sub.add_parser("mark", help="Record ground truth for one email")
    mark_parser.add_argument("email_id")
    mark_parser.add_argument("--correct-label", required=True, choices=LABELS)
    mark_parser.add_argument("--note", default=None)
    mark_parser.set_defaults(func=cmd_mark)

    report_parser = sub.add_parser("report", help="Print confusion matrix + mismatch breakdown")
    report_parser.set_defaults(func=cmd_report)

    args = parser.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
```

- [ ] **Step 2: Manual test against Task 2's sample output**

Using an id produced by Task 2 Step 3's 50-record smoke test (substitute a real id from `gmail_export/segregated_emails/*/`):

```bash
cd "/Users/adityarawal/A-B/Projects/Codes/Finance Tools/Dinero/dinero-app"
python3 gmail_export/review.py mark 19f4ca2656a60688 --correct-label transaction --note "smoke test"
cat gmail_export/review_log.jsonl
python3 gmail_export/review.py report
```

Expected: `mark` prints `Marked 19f4ca2656a60688: predicted=... correct=transaction`, `review_log.jsonl` has exactly one well-formed JSON line, and `report` prints a confusion matrix with that one entry counted (on the diagonal if predicted matched, or in the mismatch breakdown otherwise). Then test the not-found path:

```bash
python3 gmail_export/review.py mark does_not_exist --correct-label transaction
```

Expected: exits non-zero with `email_id 'does_not_exist' not found in any of [...]`.

- [ ] **Step 3: Commit**

```bash
git add gmail_export/review.py
git commit -m "feat(gmail-export): add review.py CLI for marking ground truth against predictions"
```

---

### Task 4: Full batch run

**Files:** none (operational task — runs Tasks 1–3's deliverables against the full dataset)

- [ ] **Step 1: Clear the sample-run scratch state so the full run starts clean**

```bash
cd "/Users/adityarawal/A-B/Projects/Codes/Finance Tools/Dinero/dinero-app"
rm -rf gmail_export/segregated_emails gmail_export/.scratch_db
```

- [ ] **Step 2: Run the full batch**

Quit the Dinero app, then:

```bash
src-tauri/target/release/email_segregate --progress-every 200
```

Expected: Keychain prompt once, progress lines roughly every 200 records to stderr, final summary line with `processed=5873` and a `labels` breakdown across `transaction` / `non_transaction` / `statement`. Runtime should be well under a minute given LLM (Layer 6) never fires and classification runs concurrently.

- [ ] **Step 3: Sanity-check the output**

```bash
find gmail_export/segregated_emails -name '*.json' | wc -l
python3 -c "
import json, glob
for label in ['transaction','non_transaction','statement']:
    n = len(glob.glob(f'gmail_export/segregated_emails/{label}/*.json'))
    print(label, n)
"
```

Expected: total file count equals 5873; label counts are all non-zero and roughly plausible (transaction alerts should be the largest bucket given the sample record was one).

No commit for this task — it produces gitignored data files only.
