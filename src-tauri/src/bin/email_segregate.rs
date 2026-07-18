use anyhow::Context;
use dinero_app_lib::db;
use dinero_app_lib::db::crypto::derive_database_key_from_base_key;
use dinero_app_lib::extraction::ladder::{run_extraction_ladder, ExtractionResult};
use dinero_app_lib::ingestion::content_classifier::{ContentClass, ContentClassifier};
use dinero_app_lib::ingestion::mime_sanitization::{sanitize_html, sanitize_plain_text};
use dinero_app_lib::ingestion::verified_senders::{SenderValidator, SenderVerificationResult};
use futures_util::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

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
    base_key_file: Option<PathBuf>,
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
        base_key_file: None,
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
            "--base-key-file" => args.base_key_file = Some(PathBuf::from(next())),
            other => panic!("unknown flag: {other}"),
        }
    }
    args
}

/// Opens the scratch DB copy. When `base_key_file` is set, this bypasses
/// `db::init_db`'s Keychain-backed `get_or_create_base_key` entirely and
/// derives the SQLCipher key from a base key the caller exported themselves
/// (via `security find-generic-password`, run by hand — this tool never
/// touches Keychain) — needed because this ad-hoc dev binary isn't signed
/// with the same identity as the installed Dinero.app, so macOS Keychain's
/// per-app ACL hides the real app's Keychain item from it (confirmed:
/// `security find-identity -v -p codesigning` finds zero valid identities on
/// this machine). Deliberately does NOT touch `dinero_app_lib::db::crypto`
/// or fall back to an env var inside the shared library — that would apply
/// to the real shipped app too, contradicting its documented "secrets live
/// only in Keychain" policy. Skips `db::init_db`'s integrity check, backup,
/// migrations, and seeding — unnecessary for a short-lived read-only copy of
/// an already-migrated, already-seeded live database.
async fn open_pool(db_path: PathBuf, base_key_file: Option<&Path>) -> anyhow::Result<deadpool_sqlite::Pool> {
    let Some(key_file) = base_key_file else {
        return Ok(db::init_db(db_path).await?);
    };

    let base_key = std::fs::read_to_string(key_file)
        .with_context(|| format!("failed to read --base-key-file {}", key_file.display()))?;
    let db_key = derive_database_key_from_base_key(base_key.trim())
        .context("failed to derive SQLCipher key from --base-key-file contents")?;

    let cfg = deadpool_sqlite::Config::new(&db_path);
    let pool = cfg
        .builder(deadpool_sqlite::Runtime::Tokio1)?
        .post_create(deadpool_sqlite::Hook::async_fn(move |conn, _metrics| {
            let key = db_key.clone();
            Box::pin(async move {
                conn.interact(move |c| {
                    c.execute_batch(&format!("PRAGMA key = '{}';", key))?;
                    c.execute_batch(
                        "PRAGMA cipher_page_size = 4096;
                         PRAGMA kdf_iter = 256000;
                         PRAGMA cipher_hmac_algorithm = HMAC_SHA512;
                         PRAGMA journal_mode = WAL;
                         PRAGMA synchronous = NORMAL;
                         PRAGMA foreign_keys = ON;
                         PRAGMA busy_timeout = 5000;",
                    )?;
                    Ok::<(), rusqlite::Error>(())
                })
                .await
                .map_err(|e| deadpool_sqlite::HookError::Message(e.to_string().into()))?
                .map_err(|e| deadpool_sqlite::HookError::Message(e.to_string().into()))?;
                Ok(())
            })
        }))
        .build()?;

    // Prove the key actually decrypts before handing the pool back — a wrong
    // key silently "succeeds" at PRAGMA key and only fails on first real read.
    let conn = pool.get().await?;
    conn.interact(|c| c.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get::<_, i64>(0)))
        .await
        .map_err(|e| anyhow::anyhow!("interact error verifying key: {e}"))?
        .context("base key from --base-key-file did not decrypt the database")?;

    Ok(pool)
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

    let pool = open_pool(db_path, args.base_key_file.as_deref())
        .await
        .expect("failed to open scratch database copy");
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
