//! Processes one message end to end.
//!
//! The per-message pipeline: extract metadata, sanitise the body, classify it,
//! run extraction, and emit an observation, a mandate event, or nothing at all.
//!
//! Its result type distinguishes those outcomes explicitly, because "not a
//! financial message" is a successful, expected result rather than a failure --
//! most of a mailbox is not financial, and treating that as an error would make
//! the failure counts meaningless.
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
    EnqueuedForEnrichment,
}

pub struct MessageProcessor;

/// The shared sender validator, built once.
///
/// Holds the compiled registry, so it is not rebuilt per message during a scan.
pub(crate) fn get_sender_validator() -> &'static SenderValidator {
    static VALIDATOR: OnceLock<SenderValidator> = OnceLock::new();
    VALIDATOR.get_or_init(SenderValidator::new)
}

impl MessageProcessor {
    /// Processes one message end to end.
    ///
    /// The per-message pipeline: verify the sender, sanitise the body, classify the
    /// content, extract, and emit an observation, a mandate event, or nothing.
    ///
    /// Returning "not financial" is a success, not a failure -- most of a mailbox is
    /// not financial, and counting that as an error would make the failure statistics
    /// meaningless.
    pub async fn process_message(
        pool: &Pool,
        client: &GmailClient,
        message_id: &str,
        app_dir: Option<std::path::PathBuf>,
        llm_eligible: bool,
        layer6_tx: Option<tokio::sync::mpsc::Sender<crate::ingestion::queues::Layer6Job>>,
        scan_batcher: Option<&ScanBatcherHandle>,
    ) -> Result<Option<ProcessResult>> {
        let metadata_msg = client
            .fetch_message(message_id, FetchFormat::Metadata)
            .await?;

        let sender_domain = Self::extract_sender_domain(&metadata_msg);

        let (approved_senders, bank_overrides) = match &sender_domain {
            Some(_domain) => {
                let conn = pool.get().await?;
                conn.interact(move |c| {
                    let approved = crate::db::sender_reputation::select_approved_domains(c)
                        .unwrap_or_default();
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

        if let Some(domain) = &sender_domain {
            let tag = Self::classification_tag(&gate_result).to_string();
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

        let metadata_subject = Self::header_value(&metadata_msg, "subject");
        let metadata_snippet = metadata_msg.snippet.clone().unwrap_or_default();
        let fast_class = ContentClassifier::classify(&metadata_subject, &metadata_snippet);
        if !Self::content_class_may_be_transactional(&fast_class) {
            crate::ingestion::gmail_telemetry::gmail_telemetry().record_gate_rejection("gate2a");
            let reason = format!("gate2a_reject_{:?}", fast_class);
            if matches!(fast_class, ContentClass::Noise | ContentClass::Unknown) {
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

        let full_msg = client.fetch_message(message_id, FetchFormat::Full).await?;

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
                    let event_type = if content_class == ContentClass::MandateRegistration {
                        MandateEventType::Registration
                    } else {
                        MandateEventType::Cancellation
                    };
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
                    let internal_date_seconds =
                        Self::internal_date_fallback(&full_msg.internal_date);
                    let mut _layer6_timed_out_unused = false;
                    let extracted_data = crate::extraction::ladder::run_extraction_ladder(
                        pool,
                        &current_bank_name,
                        body_text,
                        app_dir.clone(),
                        false,
                        internal_date_seconds,
                        &mut _layer6_timed_out_unused,
                        None,
                    )
                    .await
                    .unwrap_or(None);

                    if let Some(mut obs) = extracted_data {
                        if obs.event_time.is_none() {
                            obs.event_time = Self::internal_date_fallback(&full_msg.internal_date);
                        }

                        Self::apply_balance_update_placeholder(&content_class, &mut obs);

                        if let Some(dest_account) =
                            Self::self_transfer_destination_account(obs.merchant_raw.as_deref())
                        {
                            obs.merchant_raw =
                                Some(format!("Internal Transfer (A/c {dest_account})"));
                        }

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
                            let ids = Self::record_unassigned_transaction(
                                pool,
                                message_id,
                                obs.clone(),
                                body_text,
                                Some(&email_meta),
                                reason,
                            )
                            .await?;
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

    /// Falls back to the message's own timestamp when no date was extracted.
    ///
    /// The delivery time is a reasonable approximation for an alert, which banks send
    /// within moments of the transaction.
    pub(crate) fn internal_date_fallback(internal_date: &Option<String>) -> Option<i64> {
        internal_date
            .as_ref()
            .and_then(|s| s.parse::<i64>().ok())
            .map(|ts_millis| ts_millis / 1000)
    }

    /// Detects a self-transfer and returns the destination account.
    ///
    /// Money moved between the user's own accounts is not spending, so recognising it
    /// prevents an internal transfer inflating the spending totals.
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

    /// Records a balance-only update as a placeholder observation.
    ///
    /// Some alerts report a balance without any transaction. Keeping the balance is
    /// still useful, so it is stored without fabricating a transaction to attach it to.
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

    /// Gate 3: whether extraction recovered enough to record a transaction.
    ///
    /// Two ways to pass. Either the full trio of amount, counterparty and instrument
    /// is present, or the message carries a balance -- a balance-only alert is
    /// legitimate data even though it describes no transaction.
    ///
    /// Failing this gate is what routes an observation to the unassigned queue rather
    /// than into the ledger.
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

    /// Names precisely which part of gate 3 failed.
    ///
    /// Ordered by dependency: a missing amount is reported before a missing
    /// counterparty, because the amount is the more fundamental absence. The specific
    /// reason drives the diagnosis the user is shown in the unassigned queue.
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

    /// Gate 2: verifies sender identity from message headers.
    ///
    /// Missing headers are an immediate rejection -- authenticity cannot be
    /// established without them, and an unverifiable sender is treated as untrusted
    /// rather than given the benefit of the doubt.
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

        let verify_result =
            crate::ingestion::auth_results::apply_auth_results_check(verify_result, headers);

        Self::apply_bank_override(verify_result, &domain, bank_overrides)
    }

    /// Applies a user-configured sender-to-bank override.
    ///
    /// The manual correction path, which takes precedence over automatic
    /// identification because the user has explicitly stated the truth.
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

    /// Extracts the sending domain from a message.
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

    /// Parses an address and domain out of a header value.
    ///
    /// Strips a leading `From:`/`To:` label, which appears when the value comes from
    /// a forwarded or re-serialised message rather than a raw header.
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

    /// Whether a content class could plausibly describe a transaction.
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

    /// Records a message dismissed as noise, so a rescan skips it cheaply.
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

    /// Reads one header value from a message, or empty if absent.
    pub(crate) fn header_value(msg: &Message, name: &str) -> String {
        msg.payload
            .as_ref()
            .and_then(|p| p.headers.as_ref())
            .and_then(|hs| hs.iter().find(|h| h.name.eq_ignore_ascii_case(name)))
            .map(|h| h.value.clone())
            .unwrap_or_default()
    }

    /// Short tag naming the verification outcome, for logs and telemetry.
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

    /// Whether the subject alone suggests a transaction.
    ///
    /// A cheap pre-filter applied before the body is fetched, since retrieving full
    /// message bodies is the expensive part of a scan.
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

    /// Splits a From header into display name and address.
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

    /// Records why a message was rejected, feeding sender reputation.
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

    /// Records a transaction that could not be attributed to an instrument.
    ///
    /// Held for the user to resolve rather than guessed into an account, which would
    /// silently corrupt that account's balance.
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

    /// Appends a line to the scan log, for diagnosing a scan after the fact.
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
