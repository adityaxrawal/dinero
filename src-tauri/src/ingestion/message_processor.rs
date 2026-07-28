use crate::db::audit_log::{self, AuditLogRow};
use crate::ingestion::content_classifier::{ContentClass, ContentClassifier};
use crate::ingestion::gmail_client::{FetchFormat, GmailClient, Message};
use crate::ingestion::mime_sanitization::{extract_body_and_attachments, ExtractedMessage};
use crate::ingestion::verified_senders::{SenderValidator, SenderVerificationResult};
use anyhow::Result;
use chrono::Utc;
use deadpool_sqlite::Pool;
use serde_json::json;
use std::sync::OnceLock;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

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
    /// Sanitized-for-display original HTML body (see
    /// `mime_sanitization::sanitize_html_for_display`) -- `None` when the
    /// message had no `text/html` part, in which case the caller falls back
    /// to rendering `snippet` as plain text.
    pub html: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProcessResult {
    TransactionAlert(
        ExtractedMessage,
        Box<crate::extraction::ladder::ExtractionResult>,
        /// Sanitized-for-display original HTML body (same value as
        /// `EmailMetadata::html`) -- carried alongside the extraction result
        /// so the Evidence tab can render the transaction email's real
        /// layout/CSS as it appeared in Gmail, not just the plain-text body
        /// `raw_payload_json` already stores. `None` when the message had
        /// no `text/html` part.
        Option<String>,
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

fn get_sender_validator() -> &'static SenderValidator {
    static VALIDATOR: OnceLock<SenderValidator> = OnceLock::new();
    VALIDATOR.get_or_init(SenderValidator::new)
}

/// Doc 30 TASK-GMAIL-002's metadata-first guarantee ("a rejected sender
/// never triggers a full-body fetch") must hold by default even in debug
/// builds -- a bare `cfg!(debug_assertions)` check would silently spend a
/// full Gmail API call on every single rejected message (spam, newsletters,
/// ...) in every local dev session, never just when someone actually wants
/// to inspect a rejection's full content in the dev-review UI. Opt-in via
/// `DINERO_DEV_REVIEW_FULL_FETCH=1`, cached once per process.
fn dev_review_full_fetch_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        cfg!(debug_assertions)
            && std::env::var("DINERO_DEV_REVIEW_FULL_FETCH")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false)
    })
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
    ) -> Result<Option<ProcessResult>> {
        // 1. Fetch metadata first (fast, low bandwidth)
        let metadata_msg = client
            .fetch_message(message_id, FetchFormat::Metadata)
            .await?;

        // 2. Sender Verification Gate
        let sender_domain = Self::extract_sender_domain(&metadata_msg);

        // Reputation/approved-senders lookups are best-effort: a DB error
        // here must fall back to the conservative defaults (no prior
        // history, no approved override) rather than fail the whole
        // message -- Gate 1's string/auth checks still run either way.
        let (domain_previously_seen, approved_senders) = match &sender_domain {
            Some(domain) => {
                let domain_owned = domain.clone();
                let conn = pool.get().await?;
                conn.interact(move |c| {
                    let seen = crate::db::sender_reputation::has_prior_sighting(c, &domain_owned)
                        .unwrap_or(false);
                    let approved = crate::db::sender_reputation::select_approved_domains(c)
                        .unwrap_or_default();
                    (seen, approved)
                })
                .await
                .unwrap_or((false, Vec::new()))
            }
            None => (false, Vec::new()),
        };

        let gate_result =
            Self::evaluate_metadata_gate(&metadata_msg, domain_previously_seen, &approved_senders);

        // Best-effort history/learning-loop bookkeeping -- never blocks or
        // fails the gate decision itself, only records it for next time.
        if let Some(domain) = &sender_domain {
            let domain_owned = domain.clone();
            let tag = Self::classification_tag(&gate_result).to_string();
            let subject_looks_like_transaction =
                Self::subject_looks_like_transaction(&metadata_msg);
            let gate_result_owned = gate_result.clone();
            if let Ok(conn) = pool.get().await {
                let _ = conn
                    .interact(move |c| -> anyhow::Result<()> {
                        crate::db::sender_reputation::record_sighting(c, &domain_owned, &tag)?;
                        // Only flag rejections that plausibly look like a
                        // real bank alert (transaction-shaped subject) for
                        // human review -- otherwise every random rejected
                        // domain (marketing spam, unrelated mail) would
                        // flood the promotion queue.
                        if matches!(
                            gate_result_owned,
                            SenderVerificationResult::UnverifiedReject(_)
                                | SenderVerificationResult::SpoofReject(_)
                        ) && subject_looks_like_transaction
                        {
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

        let gate1_variant = crate::dev_review::gate1_variant_name(&gate_result);
        let current_bank_name = match gate_result {
            SenderVerificationResult::VerifiedTransactionCandidate(bank_name)
            | SenderVerificationResult::VerifiedStatementCandidate(bank_name) => {
                // Passes gate, proceed to fetch full body
                bank_name
            }
            SenderVerificationResult::VerifiedNoise => {
                crate::ingestion::gmail_telemetry::gmail_telemetry().record_gate_rejection("gate1");
                let dev_msg = if dev_review_full_fetch_enabled() {
                    client.fetch_message(message_id, crate::ingestion::gmail_client::FetchFormat::Full).await.unwrap_or_else(|_| metadata_msg.clone())
                } else {
                    metadata_msg.clone()
                };
                crate::dev_review::record(
                    "non_transaction",
                    serde_json::to_value(&dev_msg).unwrap_or_default(),
                    Some(gate1_variant),
                    None,
                    None,
                    None,
                    Some("gate1_verified_noise"),
                    Vec::new(),
                );
                Self::append_to_scan_log(
                    message_id,
                    "REJECTED",
                    "gate1_verified_noise",
                    Some(&serde_json::to_value(&metadata_msg).unwrap_or_default()),
                    None,
                )
                .await;
                tracing::info!("msg_id='{}' rejected by Gate 1: gate1_verified_noise", message_id);
                return Ok(None);
            }
            SenderVerificationResult::UnverifiedReject(reason)
            | SenderVerificationResult::SpoofReject(reason) => {
                // Log to audit log and return None
                crate::ingestion::gmail_telemetry::gmail_telemetry().record_gate_rejection("gate1");
                Self::log_rejection(pool, message_id, &reason).await?;
                let dev_msg = if dev_review_full_fetch_enabled() {
                    client.fetch_message(message_id, crate::ingestion::gmail_client::FetchFormat::Full).await.unwrap_or_else(|_| metadata_msg.clone())
                } else {
                    metadata_msg.clone()
                };
                crate::dev_review::record(
                    "ignored",
                    serde_json::to_value(&dev_msg).unwrap_or_default(),
                    Some(gate1_variant),
                    None,
                    None,
                    None,
                    Some(&reason),
                    Vec::new(),
                );
                Self::append_to_scan_log(
                    message_id,
                    "REJECTED",
                    &reason,
                    Some(&serde_json::to_value(&metadata_msg).unwrap_or_default()),
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
            crate::dev_review::record(
                "non_transaction",
                serde_json::to_value(&metadata_msg).unwrap_or_default(),
                Some(gate1_variant),
                Some(&current_bank_name),
                Some(&format!("{fast_class:?}")),
                None,
                Some(&reason),
                Vec::new(),
            );
            if matches!(fast_class, ContentClass::Noise | ContentClass::Unknown) {
                // Weaker signal than the full-body Gate 2 (snippet, not the
                // whole email) -- an even stronger case for the recoverable
                // Ignored table over a hard discard (spec optimization #5).
                Self::record_ignored_noise(
                    pool,
                    message_id,
                    Some(&current_bank_name),
                    &metadata_subject,
                    &metadata_snippet,
                    &reason,
                )
                .await;
            } else {
                Self::log_rejection(pool, message_id, &reason).await?;
            }
            Self::append_to_scan_log(
                message_id,
                "REJECTED",
                &reason,
                Some(&serde_json::to_value(&metadata_msg).unwrap_or_default()),
                None,
            )
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

        let email_meta = EmailMetadata {
            sender,
            recipient,
            subject: subject.clone(),
            date,
            snippet: snippet_text,
            html: html_for_display,
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
                    Self::log_rejection(pool, message_id, &reason).await?;
                    crate::dev_review::record(
                        "non_transaction",
                        serde_json::to_value(&full_msg).unwrap_or_default(),
                        Some(gate1_variant),
                        Some(&current_bank_name),
                        Some(&format!("{content_class:?}")),
                        None,
                        Some(&reason),
                        Vec::new(),
                    );
                    Self::append_to_scan_log(
                        message_id,
                        "REJECTED",
                        &reason,
                        Some(&serde_json::to_value(&full_msg).unwrap_or_default()),
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
                        message_id,
                        Some(&current_bank_name),
                        &subject,
                        &email_meta.snippet,
                        &reason,
                    )
                    .await;
                    crate::dev_review::record(
                        "non_transaction",
                        serde_json::to_value(&full_msg).unwrap_or_default(),
                        Some(gate1_variant),
                        Some(&current_bank_name),
                        Some(&format!("{content_class:?}")),
                        None,
                        Some(&reason),
                        Vec::new(),
                    );
                    Self::append_to_scan_log(
                        message_id,
                        "REJECTED",
                        &reason,
                        Some(&serde_json::to_value(&full_msg).unwrap_or_default()),
                        Some(body_text),
                    )
                    .await;
                    tracing::info!("msg_id='{}' rejected by Gate 2: {}", message_id, reason);
                    return Ok(None);
                }
                ContentClass::StatementEmail => {
                    let attachment_names: Vec<String> = extracted
                        .pdf_attachments
                        .iter()
                        .map(|a| a.filename.clone())
                        .collect();
                    crate::dev_review::record(
                        "statement",
                        serde_json::to_value(&full_msg).unwrap_or_default(),
                        Some(gate1_variant),
                        Some(&current_bank_name),
                        Some(&format!("{content_class:?}")),
                        None,
                        None,
                        attachment_names,
                    );
                    Self::append_to_scan_log(
                        message_id,
                        "SELECTED",
                        "statement_email",
                        Some(&serde_json::to_value(&full_msg).unwrap_or_default()),
                        Some(body_text),
                    )
                    .await;
                    return Ok(Some(ProcessResult::StatementEmail(extracted, Some(email_meta))));
                }
                ContentClass::MandateRegistration | ContentClass::MandateCancellation => {
                    // Mandate Queue routing
                    // (docs/superpowers/specs/2026-07-18-mandate-tracking-design.md §4.1/§4.3).
                    let event_type = if content_class == ContentClass::MandateRegistration {
                        MandateEventType::Registration
                    } else {
                        MandateEventType::Cancellation
                    };
                    match crate::extraction::mandate_extractor::extract_mandate_fields(
                        &current_bank_name,
                        body_text,
                    ) {
                        Some(extraction) => {
                            Self::append_to_scan_log(
                                message_id,
                                "SELECTED",
                                "mandate_event",
                                Some(&serde_json::to_value(&full_msg).unwrap_or_default()),
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
                            Self::log_rejection(pool, message_id, "mandate_missing_merchant")
                                .await?;
                            crate::dev_review::record(
                                "non_transaction",
                                serde_json::to_value(&full_msg).unwrap_or_default(),
                                Some(gate1_variant),
                                Some(&current_bank_name),
                                Some(&format!("{content_class:?}")),
                                None,
                                Some("mandate_missing_merchant"),
                                Vec::new(),
                            );
                            Self::append_to_scan_log(
                                message_id,
                                "REJECTED",
                                "mandate_missing_merchant",
                                Some(&serde_json::to_value(&full_msg).unwrap_or_default()),
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

                        if Self::evaluate_mandatory_field_gate(&obs) {
                            crate::dev_review::record(
                                "transaction",
                                serde_json::to_value(&full_msg).unwrap_or_default(),
                                Some(gate1_variant),
                                Some(&current_bank_name),
                                Some(&format!("{content_class:?}")),
                                Some(crate::dev_review::extraction_to_json(&obs)),
                                None,
                                Vec::new(),
                            );
                            Self::append_to_scan_log(
                                message_id,
                                "SELECTED",
                                "transaction_alert",
                                Some(&serde_json::to_value(&full_msg).unwrap_or_default()),
                                Some(body_text),
                            )
                            .await;
                            return Ok(Some(ProcessResult::TransactionAlert(
                                extracted,
                                Box::new(obs),
                                email_meta.html.clone(),
                            )));
                        } else {
                            crate::ingestion::gmail_telemetry::gmail_telemetry()
                                .record_gate_rejection("gate3");
                            let reason = Self::gate3_failure_reason(&obs);
                            Self::log_rejection(pool, message_id, reason).await?;
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
                            Self::record_unassigned_transaction(
                                pool,
                                message_id,
                                obs.clone(),
                                body_text,
                                email_meta.html.as_deref(),
                                reason,
                            )
                            .await?;
                            crate::dev_review::record(
                                "non_transaction",
                                serde_json::to_value(&full_msg).unwrap_or_default(),
                                Some(gate1_variant),
                                Some(&current_bank_name),
                                Some(&format!("{content_class:?}")),
                                Some(crate::dev_review::extraction_to_json(&obs)),
                                Some(reason),
                                Vec::new(),
                            );
                            Self::append_to_scan_log(
                                message_id,
                                "REJECTED",
                                reason,
                                Some(&serde_json::to_value(&full_msg).unwrap_or_default()),
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
                        Self::log_rejection(pool, message_id, "pending_llm_enrichment").await?;
                        let ids = Self::record_unassigned_transaction(
                            pool,
                            message_id,
                            crate::extraction::ladder::ExtractionResult::default(),
                            body_text,
                            email_meta.html.as_deref(),
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
                            };
                            if tx.send(job).await.is_err() {
                                tracing::error!(
                                    "Layer 6 Queue closed — dropping enrichment job for msg_id='{}'",
                                    message_id
                                );
                            }
                        }
                        Self::append_to_scan_log(
                            message_id,
                            "PENDING",
                            "pending_llm_enrichment",
                            Some(&serde_json::to_value(&full_msg).unwrap_or_default()),
                            Some(body_text),
                        )
                        .await;
                        return Ok(Some(ProcessResult::EnqueuedForEnrichment));
                    } else {
                        Self::log_rejection(pool, message_id, "extraction_failed").await?;
                        Self::record_unassigned_transaction(
                            pool,
                            message_id,
                            crate::extraction::ladder::ExtractionResult::default(),
                            body_text,
                            email_meta.html.as_deref(),
                            "extraction_failed",
                        )
                        .await?;
                        crate::dev_review::record(
                            "non_transaction",
                            serde_json::to_value(&full_msg).unwrap_or_default(),
                            Some(gate1_variant),
                            Some(&current_bank_name),
                            Some(&format!("{content_class:?}")),
                            None,
                            Some("extraction_failed"),
                            Vec::new(),
                        );
                        Self::append_to_scan_log(
                            message_id,
                            "REJECTED",
                            "extraction_failed",
                            Some(&serde_json::to_value(&full_msg).unwrap_or_default()),
                            Some(body_text),
                        )
                        .await;
                        return Ok(None);
                    }
                }
            }
        } else {
            crate::dev_review::record(
                "non_transaction",
                serde_json::to_value(&full_msg).unwrap_or_default(),
                Some(gate1_variant),
                Some(&current_bank_name),
                None,
                None,
                Some("no_payload"),
                Vec::new(),
            );
            Self::append_to_scan_log(
                message_id,
                "REJECTED",
                "no_payload",
                Some(&serde_json::to_value(&full_msg).unwrap_or_default()),
                None,
            )
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
    /// a parseable amount and at least one counterparty-identifying field
    /// (merchant/payee/payor), per Doc 30 TASK-GMAIL-006.
    pub(crate) fn evaluate_mandatory_field_gate(
        obs: &crate::extraction::ladder::ExtractionResult,
    ) -> bool {
        let has_amount = obs.amount_minor.is_some();
        let has_entity = obs.merchant_raw.is_some();
        let has_balance = obs.balance_after.is_some();
        (has_amount && has_entity) || has_balance
    }

    /// Structured gate3_failed reason code (missing_amount / missing_counterparty,
    /// Doc 30 TASK-GMAIL-006) for the audit trail — only meaningful to call
    /// when `evaluate_mandatory_field_gate` has already returned `false`.
    pub(crate) fn gate3_failure_reason(
        obs: &crate::extraction::ladder::ExtractionResult,
    ) -> &'static str {
        let has_amount = obs.amount_minor.is_some();
        let has_entity = obs.merchant_raw.is_some();
        match (has_amount, has_entity) {
            (false, _) => "gate3_failed:missing_amount",
            (true, false) => "gate3_failed:missing_counterparty",
            (true, true) => "gate3_failed",
        }
    }

    /// `domain_previously_seen`: whether this sender's domain has at least
    /// one prior recorded sighting (see `db::sender_reputation`) -- gates the
    /// subject-based "Unknown Bank" rescue below so a domain's very
    /// first-ever message can't be auto-accepted purely off subject-line
    /// wording, with no history yet to weigh against a spoofed subject.
    ///
    /// `approved_senders`: user-confirmed domains that repeatedly failed
    /// string-based verification (see `db::sender_reputation::PendingSenderRow`,
    /// the runtime learning-loop layer alongside the compiled-in registry
    /// JSON) -- checked before the registry so a manually-approved sender
    /// isn't still rejected by the same heuristics that flagged it originally.
    pub(crate) fn evaluate_metadata_gate(
        msg: &Message,
        domain_previously_seen: bool,
        approved_senders: &[crate::db::sender_reputation::PendingSenderRow],
    ) -> SenderVerificationResult {
        let headers = match &msg.payload {
            Some(payload) => match &payload.headers {
                Some(h) => h,
                None => return SenderVerificationResult::UnverifiedReject("No headers".into()),
            },
            None => return SenderVerificationResult::UnverifiedReject("No payload".into()),
        };

        let mut from_header = String::new();
        let mut subject_header = String::new();
        for header in headers {
            if header.name.eq_ignore_ascii_case("from") {
                from_header = header.value.clone();
            } else if header.name.eq_ignore_ascii_case("subject") {
                subject_header = header.value.clone();
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

        let verify_result = match verify_result {
            SenderVerificationResult::UnverifiedReject(_)
            | SenderVerificationResult::SpoofReject(_)
                if domain_previously_seen =>
            {
                // Requirement 6 fallback: Check subject before outright
                // rejection. Only reachable once this domain has at least
                // one prior recorded sighting -- see `domain_previously_seen`
                // doc comment above.
                let classification =
                    crate::ingestion::content_classifier::ContentClassifier::classify(
                        &subject_header,
                        "",
                    );
                match classification {
                    crate::ingestion::content_classifier::ContentClass::TransactionAlert
                    | crate::ingestion::content_classifier::ContentClass::BalanceUpdate => {
                        SenderVerificationResult::VerifiedTransactionCandidate(
                            "Unknown Bank".to_string(),
                        )
                    }
                    _ => verify_result, // return original rejection
                }
            }
            _ => verify_result,
        };

        // Cross-check against SPF/DKIM/DMARC (Doc 30 Gate 1 hardening): a
        // domain-string match alone never proves the message actually
        // originated from that domain's real infrastructure. Only ever
        // downgrades Verified* -> SpoofReject; see
        // `auth_results::apply_auth_results_check` for why it can't be used
        // to promote a rejection instead.
        crate::ingestion::auth_results::apply_auth_results_check(verify_result, headers)
    }

    /// Pulls just the sender's domain out of a *metadata*-format message's
    /// From header, for the reputation/approved-senders lookups that must
    /// happen before `evaluate_metadata_gate` runs (it needs `pool`, which
    /// that function deliberately doesn't take -- see its doc comment).
    fn extract_sender_domain(msg: &Message) -> Option<String> {
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
        message_id: &str,
        bank_name: Option<&str>,
        subject: &str,
        snippet: &str,
        reason: &str,
    ) {
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
    fn header_value(msg: &Message, name: &str) -> String {
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

    async fn log_rejection(pool: &Pool, message_id: &str, reason: &str) -> Result<()> {
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
        raw_html: Option<&str>,
        reason: &str,
    ) -> Result<Option<(String, String)>> {
        let obs_row = crate::extraction::normalization::normalize_observation(
            obs,
            "gmail_transaction",
            message_id,
            Some(body_text),
            raw_html,
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
                    let outcome = crate::db::transaction_observations::insert_observation_idempotent(
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

    async fn append_to_scan_log(
        message_id: &str,
        decision: &str,
        reason: &str,
        raw_payload: Option<&serde_json::Value>,
        body_text: Option<&str>,
    ) {
        let mut from = "N/A".to_string();
        let mut subject = "N/A".to_string();
        let mut snippet = "N/A".to_string();

        if let Some(payload_val) = raw_payload {
            if let Some(snip) = payload_val.get("snippet").and_then(|s| s.as_str()) {
                snippet = snip.to_string();
            }
            if let Some(headers) = payload_val
                .pointer("/payload/headers")
                .and_then(|h| h.as_array())
            {
                for h in headers {
                    if let (Some(name), Some(value)) = (
                        h.get("name").and_then(|n| n.as_str()),
                        h.get("value").and_then(|v| v.as_str()),
                    ) {
                        if name.eq_ignore_ascii_case("from") {
                            from = value.to_string();
                        } else if name.eq_ignore_ascii_case("subject") {
                            subject = value.to_string();
                        }
                    }
                }
            }
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
