//! Batches the incidental writes a scan generates.
//!
//! Sender sightings, rejections and ignore records are produced per message and
//! are individually trivial. Writing each one immediately would make database
//! contention, not mail fetching, the limit on scan throughput, so they are
//! accumulated and flushed together.
use anyhow::Result;
use chrono::Utc;
use deadpool_sqlite::Pool;
use serde_json::json;
use uuid::Uuid;

pub struct PendingSighting {
    pub domain: String,
    pub tag: String,
    pub is_rejection_candidate: bool,
}

pub struct PendingRejection {
    pub message_id: String,
    pub reason: String,
}

pub struct PendingIgnored {
    pub message_id: String,
    pub bank_name: Option<String>,
    pub reason: String,
    pub subject: String,
    pub snippet: String,
}

#[derive(Default)]
pub struct ScanDbBatcher {
    sightings: Vec<PendingSighting>,
    rejections: Vec<PendingRejection>,
    ignored: Vec<PendingIgnored>,
}

impl ScanDbBatcher {
    /// An empty batcher.
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues a sender sighting.
    pub fn record_sighting(&mut self, domain: &str, tag: &str, is_rejection_candidate: bool) {
        self.sightings.push(PendingSighting {
            domain: domain.to_string(),
            tag: tag.to_string(),
            is_rejection_candidate,
        });
    }

    /// Queues a rejection record.
    pub fn record_rejection(&mut self, message_id: &str, reason: &str) {
        self.rejections.push(PendingRejection {
            message_id: message_id.to_string(),
            reason: reason.to_string(),
        });
    }

    /// Queues an ignored-message record.
    pub fn record_ignored(
        &mut self,
        message_id: &str,
        bank_name: Option<&str>,
        reason: &str,
        subject: &str,
        snippet: &str,
    ) {
        self.ignored.push(PendingIgnored {
            message_id: message_id.to_string(),
            bank_name: bank_name.map(|s| s.to_string()),
            reason: reason.to_string(),
            subject: subject.to_string(),
            snippet: snippet.to_string(),
        });
    }

    /// Whether anything is queued.
    pub fn is_empty(&self) -> bool {
        self.sightings.is_empty() && self.rejections.is_empty() && self.ignored.is_empty()
    }

    /// Writes the queued records in one transaction.
    ///
    /// Batching these matters because they are produced per message and are
    /// individually trivial: written one at a time, database contention rather than
    /// mail fetching would become the limit on scan throughput.
    pub async fn flush(&mut self, pool: &Pool) -> Result<()> {
        if self.is_empty() {
            return Ok(());
        }

        let sightings = std::mem::take(&mut self.sightings);
        let rejections = std::mem::take(&mut self.rejections);
        let ignored = std::mem::take(&mut self.ignored);

        let conn = pool.get().await?;
        conn.interact(move |c| -> Result<()> {
            let tx = c
                .transaction()
                .map_err(|e| anyhow::anyhow!("TX start: {}", e))?;

            for s in &sightings {
                let _ = crate::db::sender_reputation::record_sighting(&tx, &s.domain, &s.tag);
                if s.is_rejection_candidate {
                    let _ = crate::db::sender_reputation::record_rejection_candidate(
                        &tx,
                        &Uuid::new_v4().to_string(),
                        &s.domain,
                        &s.domain,
                        "transaction_candidate",
                    );
                }
            }

            for r in &rejections {
                let row = crate::db::audit_log::AuditLogRow {
                    id: Uuid::new_v4().to_string(),
                    actor_type: Some("system".to_string()),
                    actor_id: Some("scan_prefilter".to_string()),
                    action: Some("reject_message".to_string()),
                    resource_type: Some("gmail_message".to_string()),
                    resource_id: Some(r.message_id.clone()),
                    before_json: None,
                    after_json: Some(json!({ "reason": r.reason })),
                    created_at: Utc::now(),
                };
                let _ = crate::db::audit_log::insert(&tx, &row);
            }

            for ig in &ignored {
                let row = crate::db::ignored_messages::IgnoredMessageRow::new(
                    &ig.message_id,
                    ig.bank_name.as_deref(),
                    &ig.reason,
                    &ig.subject,
                    &ig.snippet,
                );
                let _ = crate::db::ignored_messages::insert(&tx, &row);
            }

            tx.commit()
                .map_err(|e| anyhow::anyhow!("TX commit: {}", e))?;
            Ok(())
        })
        .await
        .map_err(|e| anyhow::anyhow!("Interact error: {}", e))??;

        Ok(())
    }
}
