use crate::db::audit_log::{self, AuditLogRow};
use tokio::io::AsyncWriteExt;
use crate::ingestion::content_classifier::{ContentClass, ContentClassifier};
use crate::ingestion::gmail_client::{FetchFormat, GmailClient, Message};
use crate::ingestion::mime_sanitization::{extract_body_and_attachments, ExtractedMessage};
use crate::ingestion::verified_senders::{SenderValidator, SenderVerificationResult};
use anyhow::Result;
use chrono::Utc;
use deadpool_sqlite::Pool;
use serde_json::json;
use std::sync::OnceLock;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessResult {
    TransactionAlert(
        ExtractedMessage,
        Box<crate::extraction::ladder::ExtractionResult>,
    ),
    StatementEmail(ExtractedMessage),
}

pub struct MessageProcessor;

fn get_sender_validator() -> &'static SenderValidator {
    static VALIDATOR: OnceLock<SenderValidator> = OnceLock::new();
    VALIDATOR.get_or_init(SenderValidator::new)
}

impl MessageProcessor {
    /// Processes a message by first fetching its metadata to check against a sender/subject gate.
    /// If it passes the gate, it fetches the full message and extracts its contents.
    pub async fn process_message(
        pool: &Pool,
        client: &GmailClient,
        message_id: &str,
    ) -> Result<Option<ProcessResult>> {
        // 1. Fetch metadata first (fast, low bandwidth)
        let metadata_msg = client
            .fetch_message(message_id, FetchFormat::Metadata)
            .await?;

        // 2. Sender Verification Gate
        let gate_result = Self::evaluate_metadata_gate(&metadata_msg);

        let current_bank_name = match gate_result {
            SenderVerificationResult::VerifiedTransactionCandidate(bank_name)
            | SenderVerificationResult::VerifiedStatementCandidate(bank_name) => {
                // Passes gate, proceed to fetch full body
                bank_name
            }
            SenderVerificationResult::VerifiedNoise => {
                Self::append_to_scan_log(message_id, "REJECTED", "gate1_verified_noise", Some(&serde_json::to_value(&metadata_msg).unwrap_or_default()), None).await;
                return Ok(None);
            }
            SenderVerificationResult::UnverifiedReject(reason)
            | SenderVerificationResult::SpoofReject(reason) => {
                // Log to audit log and return None
                Self::log_rejection(pool, message_id, &reason).await?;
                Self::append_to_scan_log(message_id, "REJECTED", &reason, Some(&serde_json::to_value(&metadata_msg).unwrap_or_default()), None).await;
                return Ok(None);
            }
        };

        // 3. If gate passes, fetch full body
        let full_msg = client.fetch_message(message_id, FetchFormat::Full).await?;

        // Extract subject for Gate 2
        let mut subject = String::new();
        if let Some(payload) = &full_msg.payload {
            if let Some(headers) = &payload.headers {
                for header in headers {
                    if header.name.eq_ignore_ascii_case("subject") {
                        subject = header.value.clone();
                        break;
                    }
                }
            }
        }

        // 4. Extract and sanitize
        if let Some(payload) = &full_msg.payload {
            let extracted = extract_body_and_attachments(payload);
            let body_text = extracted.text_body.as_deref().unwrap_or("");

            // GATE 2: Content Classification
            let content_class = ContentClassifier::classify(&subject, body_text);

            match content_class {
                ContentClass::Otp
                | ContentClass::Marketing
                | ContentClass::Kyc
                | ContentClass::Reminder => {
                    let reason = format!("gate2_reject_{:?}", content_class);
                    Self::log_rejection(
                        pool,
                        message_id,
                        &reason,
                    )
                    .await?;
                    Self::append_to_scan_log(message_id, "REJECTED", &reason, Some(&serde_json::to_value(&full_msg).unwrap_or_default()), Some(body_text)).await;
                    return Ok(None);
                }
                ContentClass::Noise | ContentClass::Unknown => {
                    let reason = format!("gate2_reject_{:?}", content_class);
                    Self::log_rejection(
                        pool,
                        message_id,
                        &reason,
                    )
                    .await?;
                    Self::append_to_scan_log(message_id, "REJECTED", &reason, Some(&serde_json::to_value(&full_msg).unwrap_or_default()), Some(body_text)).await;
                    return Ok(None);
                }
                ContentClass::StatementEmail => {
                    Self::append_to_scan_log(message_id, "SELECTED", "statement_email", Some(&serde_json::to_value(&full_msg).unwrap_or_default()), Some(body_text)).await;
                    return Ok(Some(ProcessResult::StatementEmail(extracted)));
                }
                ContentClass::TransactionAlert | ContentClass::BalanceUpdate => {
                    // GATE 3: Extraction and Mandatory Field Gate
                    let extracted_data = crate::extraction::ladder::run_extraction_ladder(
                        pool,
                        &current_bank_name,
                        body_text,
                        None, // Gracefully skip Layer 5 if app_dir is not provided
                    )
                    .await
                    .unwrap_or(None);

                    if let Some(mut obs) = extracted_data {
                        if let Some(internal_date_str) = &full_msg.internal_date {
                            if let Ok(ts_millis) = internal_date_str.parse::<i64>() {
                                obs.event_time = Some(ts_millis / 1000);
                            }
                        }

                        // If it's a pure balance update (no amount/merchant), fill defaults
                        if obs.merchant_raw.is_none() && obs.balance_after.is_some() {
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

                        if Self::evaluate_mandatory_field_gate(&obs) {
                            Self::append_to_scan_log(message_id, "SELECTED", "transaction_alert", Some(&serde_json::to_value(&full_msg).unwrap_or_default()), Some(body_text)).await;
                            return Ok(Some(ProcessResult::TransactionAlert(
                                extracted,
                                Box::new(obs),
                            )));
                        } else {
                            let reason = Self::gate3_failure_reason(&obs);
                            Self::log_rejection(pool, message_id, reason).await?;
                            Self::append_to_scan_log(message_id, "REJECTED", reason, Some(&serde_json::to_value(&full_msg).unwrap_or_default()), Some(body_text)).await;
                            return Ok(None);
                        }
                    } else {
                        Self::log_rejection(pool, message_id, "extraction_failed").await?;
                        Self::append_to_scan_log(message_id, "REJECTED", "extraction_failed", Some(&serde_json::to_value(&full_msg).unwrap_or_default()), Some(body_text)).await;
                        return Ok(None);
                    }
                }
            }
        } else {
             Self::append_to_scan_log(message_id, "REJECTED", "no_payload", Some(&serde_json::to_value(&full_msg).unwrap_or_default()), None).await;
        }

        Ok(None)
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
    pub(crate) fn gate3_failure_reason(obs: &crate::extraction::ladder::ExtractionResult) -> &'static str {
        let has_amount = obs.amount_minor.is_some();
        let has_entity = obs.merchant_raw.is_some();
        match (has_amount, has_entity) {
            (false, _) => "gate3_failed:missing_amount",
            (true, false) => "gate3_failed:missing_counterparty",
            (true, true) => "gate3_failed",
        }
    }

    pub(crate) fn evaluate_metadata_gate(msg: &Message) -> SenderVerificationResult {
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

        let verify_result = get_sender_validator().verify_sender(&email, display_name.as_deref());

        match verify_result {
            SenderVerificationResult::UnverifiedReject(_) | SenderVerificationResult::SpoofReject(_) => {
                // Requirement 6 fallback: Check subject before outright rejection
                let classification = crate::ingestion::content_classifier::ContentClassifier::classify(&subject_header, "");
                match classification {
                    crate::ingestion::content_classifier::ContentClass::TransactionAlert 
                    | crate::ingestion::content_classifier::ContentClass::BalanceUpdate => {
                        SenderVerificationResult::VerifiedTransactionCandidate("Unknown Bank".to_string())
                    }
                    _ => verify_result, // return original rejection
                }
            }
            _ => verify_result,
        }
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
            if let Some(headers) = payload_val.pointer("/payload/headers").and_then(|h| h.as_array()) {
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
        s.push_str("================================================================================\n\n");

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
