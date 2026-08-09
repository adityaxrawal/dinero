use crate::db::audit_log::{self, AuditLogRow};
use crate::ingestion::content_classifier::{ContentClass, ContentClassifier};
use crate::ingestion::gmail_client::{FetchFormat, GmailClient, Message};
use crate::ingestion::mime_sanitization::{extract_body_and_attachments, ExtractedMessage};
use crate::ingestion::scan_db_batcher::ScanDbBatcher;
use crate::ingestion::verified_senders::{SenderValidator, SenderVerificationResult};
use anyhow::Result;
use chrono::Utc;
use deadpool_sqlite::Pool;
use regex::Regex;
use serde_json::json;
use std::sync::{Arc, OnceLock};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use uuid::Uuid;

/// Doc 2026-07-28 mail scan performance: `Some` only for the historical-scan
/// call path (`historical_scan.rs`) -- live polling (`polling.rs`) processes
/// one message at a time so batching buys it nothing and it keeps passing
/// `None`, preserving today's immediate-write behavior there unchanged.
pub type ScanBatcherHandle = Arc<Mutex<ScanDbBatcher>>;

#[derive(Debug, Clone, PartialEq)]
pub enum MandateEventType {
    Registration,
    Cancellation,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EmailMetadata {
    pub sender: String,
    pub recipient: String,
    pub subject: String,
    pub date: String,
    pub snippet: String,
    pub html: Option<String>,
    pub sender_email: Option<String>,
    pub sender_domain: Option<String>,
    pub recipient_email: Option<String>,
    pub recipient_domain: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProcessResult {
    /// audit_03 #7: this used to carry a third `Option<String>` holding the
    /// sanitized HTML body — a verbatim `email_meta.html.clone()`, i.e. a
    /// second copy of a string the `EmailMetadata` in the fourth slot already
    /// owns. Nothing ever read it: `raw_payload_json`'s `"html"` key is built
    /// from `email_meta.html` in `normalize_observation`. Removed, which drops
    /// one full copy of every email's HTML (200–500 KB for a complex bank
    /// template) from both this value and the 256-deep Transaction Queue it
    /// was carried into.
    TransactionAlert(
        ExtractedMessage,
        Box<crate::extraction::ladder::ExtractionResult>,
        EmailMetadata,
    ),
    StatementEmail(ExtractedMessage, Option<EmailMetadata>),
    MandateEvent(
        ExtractedMessage,
        crate::extraction::mandate_extractor::MandateExtraction,
        MandateEventType,
    ),
    /// Layers 1-5 failed, this machine is LLM-eligible, and a `Layer6Job`
    /// was enqueued for background enrichment (Doc 2026-07-26 mail scan
    /// performance) — distinct from a hard `Ok(None)` rejection so callers
    /// (`historical_scan.rs`) can track it separately from `non_financial`.
    /// Carries no data: the observation/`unassigned_transactions` rows are
    /// already persisted by the time this is returned.
    EnqueuedForEnrichment,
}

pub struct MessageProcessor;

pub(crate) fn get_sender_validator() -> &'static SenderValidator {
    static VALIDATOR: OnceLock<SenderValidator> = OnceLock::new();
    VALIDATOR.get_or_init(SenderValidator::new)
}

impl MessageProcessor {
    /// Processes a message by first fetching its metadata to check against a sender/subject gate.
    /// If it passes the gate, it fetches the full message and extracts its contents.
    /// `layer6_tx`: when Layers 1-5 fail and this machine is LLM-eligible, a
    /// `Layer6Job` is enqueued onto this channel instead of awaiting Layer 6
    /// inline (Doc 2026-07-26 mail scan performance) — `None` when the
    /// caller has no queue to enqueue onto (e.g. a plain extraction-only
    /// test), in which case the message is still recorded to
    /// `unassigned_transactions` but no background enrichment happens for it.
    pub async fn process_message(
        pool: &Pool,
        client: &GmailClient,
        message_id: &str,
        app_dir: Option<std::path::PathBuf>,
        llm_eligible: bool,
        layer6_tx: Option<tokio::sync::mpsc::Sender<crate::ingestion::queues::Layer6Job>>,
        scan_batcher: Option<&ScanBatcherHandle>,
    ) -> Result<Option<ProcessResult>> {
        // 1. Fetch metadata first (fast, low bandwidth)
        let metadata_msg = client
            .fetch_message(message_id, FetchFormat::Metadata)
            .await?;

        // 2. Sender Verification Gate
        let sender_domain = Self::extract_sender_domain(&metadata_msg);

        // Reputation/approved-senders lookups are best-effort: a DB error
        // here must fall back to the conservative defaults (no approved
        // override) rather than fail the whole message -- Gate 1's
        // string/auth checks still run either way.
        let (approved_senders, bank_overrides) = match &sender_domain {
            Some(_domain) => {
                let conn = pool.get().await?;
                conn.interact(move |c| {
                    let approved = crate::db::sender_reputation::select_approved_domains(c)
                        .unwrap_or_default();
                    // Same round trip -- this adds no pool acquisition.
                    let overrides =
                        crate::db::sender_bank_overrides::select_active(c).unwrap_or_default();
                    (approved, overrides)
                })
                .await
                .unwrap_or((Vec::new(), Vec::new()))
            }
            None => (Vec::new(), Vec::new()),
        };

        let gate_result =
            Self::evaluate_metadata_gate(&metadata_msg, &approved_senders, &bank_overrides);

        // Best-effort history/learning-loop bookkeeping -- never blocks or
        // fails the gate decision itself, only records it for next time.
        if let Some(domain) = &sender_domain {
            let tag = Self::classification_tag(&gate_result).to_string();
            // Only flag rejections that plausibly look like a real bank
            // alert (transaction-shaped subject) for human review --
            // otherwise every random rejected domain (marketing spam,
            // unrelated mail) would flood the promotion queue.
            let is_rejection_candidate = matches!(
                gate_result,
                SenderVerificationResult::UnverifiedReject(_)
                    | SenderVerificationResult::SpoofReject(_)
            ) && Self::subject_looks_like_transaction(&metadata_msg);

            if let Some(batcher) = scan_batcher {
                batcher
                    .lock()
                    .await
                    .record_sighting(domain, &tag, is_rejection_candidate);
            } else {
                let domain_owned = domain.clone();
                if let Ok(conn) = pool.get().await {
                    let _ = conn
                        .interact(move |c| -> anyhow::Result<()> {
                            crate::db::sender_reputation::record_sighting(c, &domain_owned, &tag)?;
                            if is_rejection_candidate {
                                crate::db::sender_reputation::record_rejection_candidate(
                                    c,
                                    &Uuid::new_v4().to_string(),
                                    &domain_owned,
                                    &domain_owned,
                                    "transaction_candidate",
                                )?;
                            }
                            Ok(())
                        })
                        .await;
                }
            }
        }

        let current_bank_name = match gate_result {
            SenderVerificationResult::VerifiedTransactionCandidate(bank_name)
            | SenderVerificationResult::VerifiedStatementCandidate(bank_name) => {
                // Passes gate, proceed to fetch full body
                bank_name
            }
            SenderVerificationResult::VerifiedNoise => {
                crate::ingestion::gmail_telemetry::gmail_telemetry().record_gate_rejection("gate1");
                Self::append_to_scan_log(
                    message_id,
                    "REJECTED",
                    "gate1_verified_noise",
                    Some(&metadata_msg),
                    None,
                )
                .await;
                tracing::info!(
                    "msg_id='{}' rejected by Gate 1: gate1_verified_noise",
                    message_id
                );
                return Ok(None);
            }
            SenderVerificationResult::UnverifiedReject(reason)
            | SenderVerificationResult::SpoofReject(reason) => {
                // Log to audit log and return None
                crate::ingestion::gmail_telemetry::gmail_telemetry().record_gate_rejection("gate1");
                Self::log_rejection(pool, scan_batcher, message_id, &reason).await?;
                Self::append_to_scan_log(
                    message_id,
                    "REJECTED",
                    &reason,
                    Some(&metadata_msg),
                    None,
                )
                .await;
                tracing::info!("msg_id='{}' rejected by Gate 1: {}", message_id, reason);
                return Ok(None);
            }
        };

        // GATE 2a: Fast Classify off metadata alone (Subject + snippet) --
        // spec optimization #2. Gmail's `format=metadata` response already
        // includes `snippet` at no extra cost; classifying on it before
        // paying for a `FetchFormat::Full` fetch means a verified sender's
        // 2MB promo/policy email never has its body downloaded at all.
        let metadata_subject = Self::header_value(&metadata_msg, "subject");
        let metadata_snippet = metadata_msg.snippet.clone().unwrap_or_default();
        let fast_class = ContentClassifier::classify(&metadata_subject, &metadata_snippet);
        if !Self::content_class_may_be_transactional(&fast_class) {
            crate::ingestion::gmail_telemetry::gmail_telemetry().record_gate_rejection("gate2a");
            let reason = format!("gate2a_reject_{:?}", fast_class);
            if matches!(fast_class, ContentClass::Noise | ContentClass::Unknown) {
                // Weaker signal than the full-body Gate 2 (snippet, not the
                // whole email) -- an even stronger case for the recoverable
                // Ignored table over a hard discard (spec optimization #5).
                Self::record_ignored_noise(
                    pool,
                    scan_batcher,
                    message_id,
                    Some(&current_bank_name),
                    &metadata_subject,
                    &metadata_snippet,
                    &reason,
                )
                .await;
            } else {
                Self::log_rejection(pool, scan_batcher, message_id, &reason).await?;
            }
            Self::append_to_scan_log(message_id, "REJECTED", &reason, Some(&metadata_msg), None)
                .await;
            tracing::info!("msg_id='{}' rejected by Gate 2a: {}", message_id, reason);
            return Ok(None);
        }

        // 3. If gate passes, fetch full body
        let full_msg = client.fetch_message(message_id, FetchFormat::Full).await?;

        // Extract headers and metadata
        let mut subject = String::new();
        let mut sender = String::new();
        let mut recipient = String::new();
        let mut date = String::new();

        if let Some(payload) = &full_msg.payload {
            if let Some(headers) = &payload.headers {
                for header in headers {
                    if header.name.eq_ignore_ascii_case("subject") {
                        subject = header.value.clone();
                    } else if header.name.eq_ignore_ascii_case("from") {
                        sender = header.value.clone();
                    } else if header.name.eq_ignore_ascii_case("to") {
                        recipient = header.value.clone();
                    } else if header.name.eq_ignore_ascii_case("date") {
                        date = header.value.clone();
                    }
                }
            }
        }

        // 4. Extract and sanitize
        let mut body_string = String::new();
        let extracted_opt = if let Some(payload) = &full_msg.payload {
            let ext = extract_body_and_attachments(payload);
            body_string = ext.text_body.clone().unwrap_or_default();
            Some(ext)
        } else {
            None
        };

        let snippet_text = if !body_string.trim().is_empty() {
            body_string.clone()
        } else {
            full_msg.snippet.clone().unwrap_or_default()
        };

        let html_for_display = extracted_opt
            .as_ref()
            .and_then(|ext| ext.html_body.as_deref())
            .map(crate::ingestion::mime_sanitization::sanitize_html_for_display);

        let (sender_email, sender_domain) = Self::extract_email_and_domain(&sender);
        let (recipient_email, recipient_domain) = Self::extract_email_and_domain(&recipient);

        let email_meta = EmailMetadata {
            sender,
            recipient,
            subject: subject.clone(),
            date,
            snippet: snippet_text,
            html: html_for_display,
            sender_email,
            sender_domain,
            recipient_email,
            recipient_domain,
        };

        if let Some(extracted) = extracted_opt {
            let body_text = body_string.as_str();

            // GATE 2: Content Classification
            let content_class = ContentClassifier::classify(&subject, body_text);

            match content_class {
                ContentClass::Otp
                | ContentClass::Marketing
                | ContentClass::Kyc
                | ContentClass::Reminder => {
                    crate::ingestion::gmail_telemetry::gmail_telemetry()
                        .record_gate_rejection("gate2");
                    let reason = format!("gate2_reject_{:?}", content_class);
                    Self::log_rejection(pool, scan_batcher, message_id, &reason).await?;
                    Self::append_to_scan_log(
                        message_id,
                        "REJECTED",
                        &reason,
                        Some(&full_msg),
                        Some(body_text),
                    )
                    .await;
                    tracing::info!("msg_id='{}' rejected by Gate 2: {}", message_id, reason);
                    return Ok(None);
                }
                ContentClass::Noise | ContentClass::Unknown => {
                    crate::ingestion::gmail_telemetry::gmail_telemetry()
                        .record_gate_rejection("gate2");
                    let reason = format!("gate2_reject_{:?}", content_class);
                    // Spec optimization #5: recoverable, not a hard discard.
                    Self::record_ignored_noise(
                        pool,
                        scan_batcher,
                        message_id,
                        Some(&current_bank_name),
                        &subject,
                        &email_meta.snippet,
                        &reason,
                    )
                    .await;
                    Self::append_to_scan_log(
                        message_id,
                        "REJECTED",
                        &reason,
                        Some(&full_msg),
                        Some(body_text),
                    )
                    .await;
                    tracing::info!("msg_id='{}' rejected by Gate 2: {}", message_id, reason);
                    return Ok(None);
                }
                ContentClass::StatementEmail => {
                    Self::append_to_scan_log(
                        message_id,
                        "SELECTED",
                        "statement_email",
                        Some(&full_msg),
                        Some(body_text),
                    )
                    .await;
                    return Ok(Some(ProcessResult::StatementEmail(
                        extracted,
                        Some(email_meta),
                    )));
                }
                ContentClass::MandateRegistration | ContentClass::MandateCancellation => {
                    // Mandate Queue routing
                    // (dinero-docs/design-archive/specs/2026-07-18-mandate-tracking-design.md §4.1/§4.3).
                    let event_type = if content_class == ContentClass::MandateRegistration {
                        MandateEventType::Registration
                    } else {
                        MandateEventType::Cancellation
                    };
                    // Bank-specific mandate template first, global regex set
                    // as the fallback. A bank with no `txn_type: "mandate"`
                    // pattern (or whose pattern doesn't match this body)
                    // resolves to exactly the previous behaviour.
                    let mandate_fields =
                        crate::extraction::mandate_extractor::bank_mandate_template(
                            &current_bank_name,
                            body_text,
                        )
                        .or_else(|| {
                            crate::extraction::mandate_extractor::extract_mandate_fields(
                                &current_bank_name,
                                body_text,
                            )
                        });
                    match mandate_fields {
                        Some(extraction) => {
                            Self::append_to_scan_log(
                                message_id,
                                "SELECTED",
                                "mandate_event",
                                Some(&full_msg),
                                Some(body_text),
                            )
                            .await;
                            return Ok(Some(ProcessResult::MandateEvent(
                                extracted, extraction, event_type,
                            )));
                        }
                        None => {
                            crate::ingestion::gmail_telemetry::gmail_telemetry()
                                .record_gate_rejection("gate3");
                            Self::log_rejection(
                                pool,
                                scan_batcher,
                                message_id,
                                "mandate_missing_merchant",
                            )
                            .await?;
                            Self::append_to_scan_log(
                                message_id,
                                "REJECTED",
                                "mandate_missing_merchant",
                                Some(&full_msg),
                                Some(body_text),
                            )
                            .await;
                            return Ok(None);
                        }
                    }
                }
                ContentClass::TransactionAlert | ContentClass::BalanceUpdate => {
                    // GATE 3: Extraction and Mandatory Field Gate
                    // Doc 30 TASK-TXN-005: Layer 5's ±3-day statement-row
                    // search window needs an anchor date even when the email
                    // body itself yields none — Gmail's internalDate is the
                    // only signal always available at this point.
                    let internal_date_seconds =
                        Self::internal_date_fallback(&full_msg.internal_date);
                    // Doc 2026-07-26 mail scan performance: always pass
                    // `false` here so `run_extraction_ladder` only ever runs
                    // Layers 1-5 (fast, regex-based) on this path — Layer 6
                    // (LLM) is never awaited inline anymore; a message that
                    // needs it is enqueued below instead of blocking this
                    // scan slot on LLM inference.
                    let mut _layer6_timed_out_unused = false;
                    let extracted_data = crate::extraction::ladder::run_extraction_ladder(
                        pool,
                        &current_bank_name,
                        body_text,
                        app_dir.clone(),
                        false,
                        internal_date_seconds,
                        &mut _layer6_timed_out_unused,
                        // Drift self-healing lives inside the Layer 6 success
                        // path, and `llm_eligible` is hardcoded `false` above,
                        // so no handle is needed here. Layer 6 now runs in the
                        // background worker instead, which does its own drift
                        // check (`queues::process_layer6_job`).
                        None,
                    )
                    .await
                    .unwrap_or(None);

                    if let Some(mut obs) = extracted_data {
                        // Doc 30 TASK-TXN-004: Gmail's internalDate is a
                        // *fallback* for Layer 3's generic date regex, not an
                        // unconditional override — Layers 1/2 already parse
                        // the real transaction date/time out of the email
                        // body, which can legitimately differ from (and is
                        // more precise than) the email's arrival timestamp.
                        if obs.event_time.is_none() {
                            obs.event_time = Self::internal_date_fallback(&full_msg.internal_date);
                        }

                        Self::apply_balance_update_placeholder(&content_class, &mut obs);

                        // A bank-template regex matching "debited from
                        // account X to account Y" against a self-transfer
                        // between the user's own accounts captures the
                        // destination account number ("account 1527") into
                        // `merchant_raw` -- not a merchant, there genuinely
                        // isn't one. Recognized here (before the anti-merchant
                        // gate below would otherwise just discard it as an
                        // implausible name) and replaced with an explicit
                        // placeholder, the same way `BalanceUpdate` already
                        // gets a synthetic "Balance Update" merchant instead
                        // of failing Gate 3's counterparty requirement.
                        if let Some(dest_account) =
                            Self::self_transfer_destination_account(obs.merchant_raw.as_deref())
                        {
                            obs.merchant_raw =
                                Some(format!("Internal Transfer (A/c {dest_account})"));
                        }

                        // Anti-merchant gate, applied *before* Gate 3 rather
                        // than at normalization time.
                        //
                        // `normalize_merchant_sync` already refuses to create a
                        // merchants row for a generic fragment, but it runs
                        // during reconciliation — by then Gate 3 has seen a
                        // non-empty `merchant_raw`, passed the transaction, and
                        // the row simply ends up permanently merchant-less with
                        // no signal that anything was wrong. Clearing it here
                        // instead means a generic capture is treated as the
                        // missing counterparty it actually is, which routes it
                        // to Layer 6 below the same way any other unresolved
                        // field does.
                        if let Some(raw) = obs.merchant_raw.clone() {
                            let cleaned =
                                crate::extraction::merchant_normalizer::strip_noise_tokens(&raw);
                            if !crate::extraction::merchant_normalizer::is_plausible_merchant_name(
                                &cleaned,
                            ) {
                                tracing::debug!(
                                    merchant_raw = %raw,
                                    "Rejected generic merchant candidate; routing as missing counterparty"
                                );
                                obs.merchant_raw = None;
                            }
                        }

                        if Self::evaluate_mandatory_field_gate(&obs) {
                            Self::append_to_scan_log(
                                message_id,
                                "SELECTED",
                                "transaction_alert",
                                Some(&full_msg),
                                Some(body_text),
                            )
                            .await;
                            return Ok(Some(ProcessResult::TransactionAlert(
                                extracted,
                                Box::new(obs),
                                email_meta.clone(),
                            )));
                        } else {
                            crate::ingestion::gmail_telemetry::gmail_telemetry()
                                .record_gate_rejection("gate3");
                            let reason = Self::gate3_failure_reason(&obs);
                            Self::log_rejection(pool, scan_batcher, message_id, reason).await?;
                            // Spec optimization #1: Gate 2 already confidently
                            // classified this as a transaction/balance-update —
                            // a Gate 3 mandatory-field miss must never be a
                            // silent discard. Salvage whatever fields were
                            // actually extracted into the Unassigned Queue,
                            // tagged with the specific missing-field reason
                            // (`gate3_failed:missing_amount` /
                            // `gate3_failed:missing_counterparty`) so the user
                            // can manually complete it instead of the
                            // transaction vanishing outright.
                            let ids = Self::record_unassigned_transaction(
                                pool,
                                message_id,
                                obs.clone(),
                                body_text,
                                Some(&email_meta),
                                reason,
                            )
                            .await?;
                            // A Gate 3 miss on the counterparty is usually the
                            // anti-merchant gate having thrown away a generic
                            // fragment ("YOUR HDFC BANK RUPAY CREDIT") rather
                            // than the email genuinely naming nobody — the
                            // amount and date parsed fine, only the merchant
                            // didn't. A miss on the instrument is the same
                            // shape of problem: the body-wide regex heuristics
                            // (`extract_instrument_signals`) didn't recognize
                            // this bank's phrasing for its own last-4/issuer.
                            // Both are exactly what Layer 6 is good at, so hand
                            // off to the same background worker the
                            // all-layers-failed path uses instead of leaving a
                            // permanently incomplete row for the user to fix by
                            // hand. `missing_amount` alone stays excluded — an
                            // LLM has nothing more to find in the source text
                            // than the regex layers already tried for a value
                            // that plainly isn't there in any numeral form.
                            // Enqueue-only: the scan slot is never blocked on
                            // inference.
                            let recoverable_by_llm = matches!(
                                reason,
                                "gate3_failed:missing_counterparty"
                                    | "gate3_failed:missing_instrument"
                            );
                            if recoverable_by_llm && llm_eligible {
                                if let (
                                    Some((observation_id, unassigned_id)),
                                    Some(dir),
                                    Some(tx),
                                ) = (ids, app_dir.clone(), layer6_tx.as_ref())
                                {
                                    let job = crate::ingestion::queues::Layer6Job {
                                        observation_id,
                                        unassigned_id,
                                        bank_name: current_bank_name.clone(),
                                        body_text: body_text.to_string(),
                                        app_dir: dir,
                                        internal_date_seconds,
                                    };
                                    crate::ingestion::queues::enqueue_layer6_job(pool, tx, job)
                                        .await;
                                }
                            }
                            Self::append_to_scan_log(
                                message_id,
                                "REJECTED",
                                reason,
                                Some(&full_msg),
                                Some(body_text),
                            )
                            .await;
                            return Ok(None);
                        }
                    } else if llm_eligible {
                        // Layers 1-5 failed but this machine can run Layer 6
                        // — record now (recoverable, same principle as every
                        // other unassigned path) and hand off to the
                        // background Layer 6 worker instead of blocking this
                        // scan slot on LLM inference.
                        Self::log_rejection(
                            pool,
                            scan_batcher,
                            message_id,
                            "pending_llm_enrichment",
                        )
                        .await?;
                        let ids = Self::record_unassigned_transaction(
                            pool,
                            message_id,
                            crate::extraction::ladder::ExtractionResult::default(),
                            body_text,
                            Some(&email_meta),
                            "pending_llm_enrichment",
                        )
                        .await?;
                        if let (Some((observation_id, unassigned_id)), Some(dir), Some(tx)) =
                            (ids, app_dir.clone(), layer6_tx.as_ref())
                        {
                            let job = crate::ingestion::queues::Layer6Job {
                                observation_id,
                                unassigned_id,
                                bank_name: current_bank_name.clone(),
                                body_text: body_text.to_string(),
                                app_dir: dir,
                                internal_date_seconds,
                            };
                            crate::ingestion::queues::enqueue_layer6_job(pool, tx, job).await;
                        }
                        Self::append_to_scan_log(
                            message_id,
                            "PENDING",
                            "pending_llm_enrichment",
                            Some(&full_msg),
                            Some(body_text),
                        )
                        .await;
                        return Ok(Some(ProcessResult::EnqueuedForEnrichment));
                    } else {
                        Self::log_rejection(pool, scan_batcher, message_id, "extraction_failed")
                            .await?;
                        Self::record_unassigned_transaction(
                            pool,
                            message_id,
                            crate::extraction::ladder::ExtractionResult::default(),
                            body_text,
                            Some(&email_meta),
                            "extraction_failed",
                        )
                        .await?;
                        Self::append_to_scan_log(
                            message_id,
                            "REJECTED",
                            "extraction_failed",
                            Some(&full_msg),
                            Some(body_text),
                        )
                        .await;
                        return Ok(None);
                    }
                }
            }
        } else {
            Self::append_to_scan_log(message_id, "REJECTED", "no_payload", Some(&full_msg), None)
                .await;
        }

        Ok(None)
    }

    /// Doc 30 TASK-TXN-004: parses Gmail's `internalDate` (epoch
    /// milliseconds, as a string) into epoch seconds. Only ever called when
    /// extraction itself found no date — this is a fallback, not a general
    /// date source.
    pub(crate) fn internal_date_fallback(internal_date: &Option<String>) -> Option<i64> {
        internal_date
            .as_ref()
            .and_then(|s| s.parse::<i64>().ok())
            .map(|ts_millis| ts_millis / 1000)
    }

    /// Detects a raw merchant capture that's actually the destination
    /// account of a self-transfer (e.g. HDFC's "debited from account 4691
    /// to account 1527" template captures "account 1527" into the merchant
    /// group since it has no dedicated self-transfer pattern). Returns the
    /// destination account digits so the caller can build a placeholder.
    pub(crate) fn self_transfer_destination_account(merchant_raw: Option<&str>) -> Option<String> {
        static RE: OnceLock<Regex> = OnceLock::new();
        let re = RE.get_or_init(|| {
            Regex::new(r"(?i)^(?:account|a/c|acct)\s*(?:no\.?|number|#)?\s*[Xx*\s\-.]*(\d{2,})$")
                .unwrap()
        });
        re.captures(merchant_raw?.trim())
            .and_then(|caps| caps.get(1))
            .map(|m| m.as_str().to_string())
    }

    /// A `BalanceUpdate` email is never a settled transaction, so whatever
    /// merchant/amount extraction guessed for it is discarded unconditionally
    /// -- not just filled in when absent. Without this, a balance-only email
    /// ("The available balance ... is Rs. 6,773.00 ... For real-time balance
    /// updates, call us at ...") can still mis-extract a garbage merchant
    /// ("real") and treat the balance figure as a transaction amount, since
    /// `evaluate_mandatory_field_gate` passes on `balance_after` alone
    /// regardless of what else is set. A `TransactionAlert` email that
    /// extraction genuinely returned with no merchant (but did resolve a
    /// balance) keeps the narrower, additive fallback it always had.
    pub(crate) fn apply_balance_update_placeholder(
        content_class: &ContentClass,
        obs: &mut crate::extraction::ladder::ExtractionResult,
    ) {
        if obs.balance_after.is_none() {
            return;
        }
        if *content_class == ContentClass::BalanceUpdate {
            obs.merchant_raw = Some("Balance Update".to_string());
            obs.amount_minor = Some(0);
            obs.direction = Some("unknown".to_string());
            obs.currency = Some("INR".to_string());
        } else if obs.merchant_raw.is_none() {
            obs.merchant_raw = Some("Balance Update".to_string());
            if obs.amount_minor.is_none() {
                obs.amount_minor = Some(0);
            }
            if obs.direction.is_none() {
                obs.direction = Some("unknown".to_string());
            }
            if obs.currency.is_none() {
                obs.currency = Some("INR".to_string());
            }
        }
    }

    /// Evaluates if the extracted observation passes Gate 3 (Mandatory Fields):
    /// a parseable amount, at least one counterparty-identifying field
    /// (merchant/payee/payor), and a resolvable instrument (type + issuer +
    /// masked identifier) -- per Doc 30 TASK-GMAIL-006, extended 2026-07-30
    /// so a transaction can no longer be created with a NULL instrument_id.
    /// A balance-only email stays exempt from every other mandatory field,
    /// instrument included -- it was never going to become a transaction.
    pub(crate) fn evaluate_mandatory_field_gate(
        obs: &crate::extraction::ladder::ExtractionResult,
    ) -> bool {
        let has_amount = obs.amount_minor.is_some();
        let has_entity = obs.merchant_raw.is_some();
        let has_balance = obs.balance_after.is_some();
        let has_instrument = obs.instrument_type.is_some()
            && obs.issuer_name.is_some()
            && obs.masked_identifier.is_some();
        (has_amount && has_entity && has_instrument) || has_balance
    }

    /// Structured gate3_failed reason code (missing_amount / missing_counterparty
    /// / missing_instrument, Doc 30 TASK-GMAIL-006) for the audit trail — only
    /// meaningful to call when `evaluate_mandatory_field_gate` has already
    /// returned `false`.
    pub(crate) fn gate3_failure_reason(
        obs: &crate::extraction::ladder::ExtractionResult,
    ) -> &'static str {
        let has_amount = obs.amount_minor.is_some();
        let has_entity = obs.merchant_raw.is_some();
        let has_instrument = obs.instrument_type.is_some()
            && obs.issuer_name.is_some()
            && obs.masked_identifier.is_some();
        match (has_amount, has_entity, has_instrument) {
            (false, _, _) => "gate3_failed:missing_amount",
            (true, false, _) => "gate3_failed:missing_counterparty",
            (true, true, false) => "gate3_failed:missing_instrument",
            (true, true, true) => "gate3_failed",
        }
    }

    /// `approved_senders`: user-confirmed domains that repeatedly failed
    /// string-based verification (see `db::sender_reputation::PendingSenderRow`,
    /// the runtime learning-loop layer alongside the compiled-in registry
    /// JSON) -- checked before the registry so a manually-approved sender
    /// isn't still rejected by the same heuristics that flagged it originally.
    /// `bank_overrides`: user-reported "this sender's bank is wrong"
    /// corrections. Relabel-only -- see [`Self::apply_bank_override`].
    pub(crate) fn evaluate_metadata_gate(
        msg: &Message,
        approved_senders: &[crate::db::sender_reputation::PendingSenderRow],
        bank_overrides: &[crate::db::sender_bank_overrides::SenderBankOverride],
    ) -> SenderVerificationResult {
        let headers = match &msg.payload {
            Some(payload) => match &payload.headers {
                Some(h) => h,
                None => return SenderVerificationResult::UnverifiedReject("No headers".into()),
            },
            None => return SenderVerificationResult::UnverifiedReject("No payload".into()),
        };

        let mut from_header = String::new();
        for header in headers {
            if header.name.eq_ignore_ascii_case("from") {
                from_header = header.value.clone();
            }
        }

        if from_header.trim().is_empty() {
            return SenderVerificationResult::UnverifiedReject(
                "Empty or missing From header".into(),
            );
        }

        let (email, display_name) = Self::parse_from_header(&from_header);

        let domain = email.rsplit('@').next().unwrap_or("").to_lowercase();
        let approved_match = approved_senders.iter().find(|p| p.domain == domain);

        let verify_result = if let Some(approved) = approved_match {
            match approved.classification.as_str() {
                "statement_candidate" => {
                    SenderVerificationResult::VerifiedStatementCandidate(approved.bank_name.clone())
                }
                _ => SenderVerificationResult::VerifiedTransactionCandidate(
                    approved.bank_name.clone(),
                ),
            }
        } else {
            get_sender_validator().verify_sender(&email, display_name.as_deref())
        };

        // Cross-check against SPF/DKIM/DMARC (Doc 30 Gate 1 hardening): a
        // domain-string match alone never proves the message actually
        // originated from that domain's real infrastructure. Only ever
        // downgrades Verified* -> SpoofReject; see
        // `auth_results::apply_auth_results_check` for why it can't be used
        // to promote a rejection instead.
        let verify_result =
            crate::ingestion::auth_results::apply_auth_results_check(verify_result, headers);

        // Applied last, on purpose: every spoof, typo-squat, homoglyph and
        // SPF/DKIM/DMARC check has already run and reached a verdict. This only
        // renames the bank on a verdict that was already an acceptance.
        Self::apply_bank_override(verify_result, &domain, bank_overrides)
    }

    /// Applies a user-reported bank relabel to an already-decided verification
    /// result.
    ///
    /// **Relabel only.** A rejected sender stays rejected and `VerifiedNoise`
    /// stays noise -- the override changes the *name* on a decision, never the
    /// decision. This matters because the user's report answers "which bank is
    /// this?", and treating it as an answer to "is this sender trustworthy?"
    /// would turn a one-tap naming fix into a domain whitelist.
    /// `pending_senders` remains the only path that can promote an unverified
    /// domain, and it asks that question explicitly.
    ///
    /// Nothing is lost by this restriction: a sender that never passes the gate
    /// never produces a transaction, so there is no row for a user to report a
    /// wrong bank on in the first place.
    pub(crate) fn apply_bank_override(
        result: SenderVerificationResult,
        domain: &str,
        overrides: &[crate::db::sender_bank_overrides::SenderBankOverride],
    ) -> SenderVerificationResult {
        let domain = domain.trim().to_lowercase();
        let Some(o) = overrides.iter().find(|o| o.domain == domain) else {
            return result;
        };
        match result {
            SenderVerificationResult::VerifiedTransactionCandidate(_) => {
                SenderVerificationResult::VerifiedTransactionCandidate(o.bank_name.clone())
            }
            SenderVerificationResult::VerifiedStatementCandidate(_) => {
                SenderVerificationResult::VerifiedStatementCandidate(o.bank_name.clone())
            }
            other => other,
        }
    }

    /// Pulls just the sender's domain out of a *metadata*-format message's
    /// From header, for the reputation/approved-senders lookups that must
    /// happen before `evaluate_metadata_gate` runs (it needs `pool`, which
    /// that function deliberately doesn't take -- see its doc comment).
    pub(crate) fn extract_sender_domain(msg: &Message) -> Option<String> {
        let headers = msg.payload.as_ref()?.headers.as_ref()?;
        let from_header = headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case("from"))?
            .value
            .as_str();
        let (email, _display_name) = Self::parse_from_header(from_header);
        let domain = email.rsplit('@').next()?.trim().to_lowercase();
        if domain.is_empty() {
            None
        } else {
            Some(domain)
        }
    }

    /// Extracts clean email address and domain from an email header (e.g. "From" or "To").
    /// Handles display names ("Display Name" <email@domain.com>), multiple comma-separated addresses,
    /// subdomains, bare email addresses, and invalid formats.
    pub fn extract_email_and_domain(header_val: &str) -> (Option<String>, Option<String>) {
        let mut trimmed = header_val.trim();
        if trimmed.is_empty() {
            return (None, None);
        }
        if let Some(rest) = trimmed
            .strip_prefix("From:")
            .or_else(|| trimmed.strip_prefix("from:"))
            .or_else(|| trimmed.strip_prefix("To:"))
            .or_else(|| trimmed.strip_prefix("to:"))
        {
            trimmed = rest.trim();
        }

        let first_addr = trimmed.split(',').next().unwrap_or(trimmed);
        let raw_email =
            if let (Some(start), Some(end)) = (first_addr.find('<'), first_addr.rfind('>')) {
                if start < end {
                    first_addr[start + 1..end].trim()
                } else {
                    first_addr.trim()
                }
            } else {
                first_addr.trim()
            };

        let email_clean = raw_email
            .trim_matches(|c| c == '"' || c == '\'' || c == ' ')
            .to_lowercase();
        if email_clean.is_empty() || !email_clean.contains('@') {
            return (None, None);
        }

        let parts: Vec<&str> = email_clean.rsplitn(2, '@').collect();
        if parts.len() == 2 {
            let domain_raw = parts[0]
                .trim_matches(|c| c == ']' || c == '[' || c == ' ')
                .to_lowercase();
            if !domain_raw.is_empty() {
                return (Some(email_clean), Some(domain_raw));
            }
        }

        (Some(email_clean), None)
    }

    /// Short tag persisted to `sender_reputation.last_verification_result`
    /// (and used to decide `verified_pass_count`) -- see
    /// `db::sender_reputation::record_sighting`.
    /// Spec optimization #2's Gate 2b test: "transaction, statement, or
    /// mandate" — every `ContentClass` that justifies paying for a full-body
    /// fetch. Everything else (`Noise`/`Unknown`/`Otp`/`Kyc`/`Marketing`/
    /// `Reminder`) is rejected off metadata alone.
    fn content_class_may_be_transactional(class: &ContentClass) -> bool {
        matches!(
            class,
            ContentClass::TransactionAlert
                | ContentClass::BalanceUpdate
                | ContentClass::StatementEmail
                | ContentClass::MandateRegistration
                | ContentClass::MandateCancellation
        )
    }

    /// Spec optimization #5: `Noise`/`Unknown` is Gate 2's "I don't know"
    /// bucket, not a confident rejection like `Otp`/`Marketing`/`Kyc`/
    /// `Reminder` — a heuristic misfire here must be recoverable, not a hard
    /// delete. Best-effort: a DB error recording the ignore must not fail
    /// the caller's early-return (matches `log_rejection`'s existing
    /// best-effort framing elsewhere in this file).
    async fn record_ignored_noise(
        pool: &Pool,
        scan_batcher: Option<&ScanBatcherHandle>,
        message_id: &str,
        bank_name: Option<&str>,
        subject: &str,
        snippet: &str,
        reason: &str,
    ) {
        if let Some(batcher) = scan_batcher {
            batcher
                .lock()
                .await
                .record_ignored(message_id, bank_name, reason, subject, snippet);
            return;
        }
        let row = crate::db::ignored_messages::IgnoredMessageRow::new(
            message_id, bank_name, reason, subject, snippet,
        );
        if let Ok(conn) = pool.get().await {
            let _ = conn
                .interact(move |c| crate::db::ignored_messages::insert(c, &row))
                .await;
        }
    }

    /// Metadata-format messages only carry a `headers` array on `payload`
    /// (no body) — same shape `evaluate_metadata_gate`/`extract_sender_domain`
    /// already read, factored out here since Gate 2a needs the Subject too.
    pub(crate) fn header_value(msg: &Message, name: &str) -> String {
        msg.payload
            .as_ref()
            .and_then(|p| p.headers.as_ref())
            .and_then(|hs| hs.iter().find(|h| h.name.eq_ignore_ascii_case(name)))
            .map(|h| h.value.clone())
            .unwrap_or_default()
    }

    fn classification_tag(result: &SenderVerificationResult) -> &'static str {
        match result {
            SenderVerificationResult::VerifiedTransactionCandidate(_) => {
                "verified_transaction_candidate"
            }
            SenderVerificationResult::VerifiedStatementCandidate(_) => {
                "verified_statement_candidate"
            }
            SenderVerificationResult::VerifiedNoise => "verified_noise",
            SenderVerificationResult::UnverifiedReject(_) => "unverified_reject",
            SenderVerificationResult::SpoofReject(_) => "spoof_reject",
        }
    }

    /// Whether the metadata-format message's Subject line alone looks like a
    /// transaction alert -- the same signal `evaluate_metadata_gate`'s
    /// subject-rescue fallback uses, re-derived here (not passed out of that
    /// function) so a rejected sender's pending-promotion candidacy
    /// (`db::sender_reputation::record_rejection_candidate`) only ever
    /// covers domains that plausibly look like a real bank alert, not every
    /// rejected domain indiscriminately.
    fn subject_looks_like_transaction(msg: &Message) -> bool {
        let Some(headers) = msg.payload.as_ref().and_then(|p| p.headers.as_ref()) else {
            return false;
        };
        let subject = headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case("subject"))
            .map(|h| h.value.as_str())
            .unwrap_or("");
        matches!(
            crate::ingestion::content_classifier::ContentClassifier::classify(subject, ""),
            crate::ingestion::content_classifier::ContentClass::TransactionAlert
                | crate::ingestion::content_classifier::ContentClass::BalanceUpdate
        )
    }

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

        // No angle brackets, assume the whole string is the email
        (from.trim().to_string(), None)
    }

    async fn log_rejection(
        pool: &Pool,
        scan_batcher: Option<&ScanBatcherHandle>,
        message_id: &str,
        reason: &str,
    ) -> Result<()> {
        if let Some(batcher) = scan_batcher {
            batcher.lock().await.record_rejection(message_id, reason);
            return Ok(());
        }
        let row = AuditLogRow {
            id: Uuid::new_v4().to_string(),
            actor_type: Some("system".to_string()),
            actor_id: Some("message_processor".to_string()),
            action: Some("reject_message".to_string()),
            resource_type: Some("gmail_message".to_string()),
            resource_id: Some(message_id.to_string()),
            before_json: None,
            after_json: Some(json!({ "reason": reason })),
            created_at: Utc::now(),
        };

        let conn = pool.get().await?;
        conn.interact(move |c| audit_log::insert(c, &row))
            .await
            .map_err(|e| anyhow::anyhow!("Interact error: {}", e))??;

        Ok(())
    }

    /// Doc 30 TASK-TXN-001 / spec optimization #1: "never silently drop data
    /// Gate 2 believed was a transaction." Called both when extraction found
    /// nothing at all (`obs = ExtractionResult::default()`) and when Gate 3's
    /// mandatory-field check rejected a *partial* extraction (missing amount
    /// or counterparty) — the latter previously bypassed this function
    /// entirely and was audit-logged only, discarding whatever fields *were*
    /// found. Reuses `normalize_observation` (the same obs -> row conversion
    /// the successful-extraction path uses via
    /// `queues::process_transaction_job`) so a partial extraction keeps
    /// every field it actually resolved, not just the raw body.
    pub(crate) async fn record_unassigned_transaction(
        pool: &Pool,
        message_id: &str,
        obs: crate::extraction::ladder::ExtractionResult,
        body_text: &str,
        email_meta: Option<&EmailMetadata>,
        reason: &str,
    ) -> Result<Option<(String, String)>> {
        let obs_row = crate::extraction::normalization::normalize_observation(
            obs,
            "gmail_transaction",
            message_id,
            Some(body_text),
            email_meta,
        );
        let observation_id = obs_row.id.clone();
        let unassigned_id = Uuid::new_v4().to_string();
        let reason_owned = reason.to_string();

        let conn = pool.get().await?;
        let inserted = conn
            .interact({
                let unassigned_id = unassigned_id.clone();
                let observation_id = observation_id.clone();
                move |c| -> Result<bool> {
                    use crate::db::transaction_observations::InsertObservationOutcome;
                    let outcome =
                        crate::db::transaction_observations::insert_observation_idempotent(
                            c, &obs_row,
                        )?;
                    if let InsertObservationOutcome::Inserted = outcome {
                        crate::db::unassigned_transactions::insert(
                            c,
                            &crate::db::unassigned_transactions::UnassignedTransactionRow {
                                id: unassigned_id,
                                observation_id,
                                reason: reason_owned,
                                status: "open".to_string(),
                                created_at: None,
                            },
                        )?;
                        Ok(true)
                    } else {
                        Ok(false)
                    }
                }
            })
            .await
            .map_err(|e| anyhow::anyhow!("Interact error: {}", e))??;

        Ok(if inserted {
            Some((observation_id, unassigned_id))
        } else {
            None
        })
    }

    /// Takes the `Message` itself rather than a `serde_json::Value`: only
    /// `snippet` and two headers are ever read out of it, and serialising the
    /// whole message (base64 body included) per call just to read three fields
    /// was pure waste on every message the scan touched.
    async fn append_to_scan_log(
        message_id: &str,
        decision: &str,
        reason: &str,
        raw_msg: Option<&Message>,
        body_text: Option<&str>,
    ) {
        let mut from = "N/A".to_string();
        let mut subject = "N/A".to_string();
        let mut snippet = "N/A".to_string();

        if let Some(msg) = raw_msg {
            if let Some(snip) = &msg.snippet {
                snippet = snip.clone();
            }
            from = Self::header_value(msg, "from");
            subject = Self::header_value(msg, "subject");
        }

        let timestamp = chrono::Utc::now().to_rfc3339();
        let mut s = format!(
            "================================================================================\n\
             Timestamp  : {}\n\
             Message ID : {}\n\
             Decision   : {}\n\
             Reason     : {}\n\
             From       : {}\n\
             Subject    : {}\n\
             --------------------------------------------------------------------------------\n\
             Snippet    : {}\n",
            timestamp, message_id, decision, reason, from, subject, snippet
        );

        if let Some(body) = body_text {
            s.push_str("--------------------------------------------------------------------------------\nBody Preview:\n");
            let preview: String = body.chars().take(500).collect();
            s.push_str(&preview);
            if body.len() > 500 {
                s.push_str("... [TRUNCATED]\n");
            } else {
                s.push('\n');
            }
        }
        s.push_str(
            "================================================================================\n\n",
        );

        let path_str = if decision == "SELECTED" {
            "email_scan_selected.log"
        } else {
            "email_scan_rejected.log"
        };
        let path = std::path::PathBuf::from(path_str);
        if let Ok(mut file) = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await
        {
            let _ = file.write_all(s.as_bytes()).await;
        }
    }
}
