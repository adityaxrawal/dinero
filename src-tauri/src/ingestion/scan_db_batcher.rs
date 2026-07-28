/// Batch DB writer for historical-scan bookkeeping records.
///
/// During a historical scan the pre-filter phase (Phase 1) examines every
/// message and records:
///   - a sender-domain sighting (one row per message in `sender_reputation`)
///   - an audit-log rejection row (for messages that fail Gate 1 / Gate 2a)
///   - an ignored-message row (for Noise/Unknown Gate 2a rejections)
///
/// Writing these one-at-a-time (one `pool.get() + conn.interact()` per
/// message) was a major bottleneck: for 897 ignored emails that's ~2,700 DB
/// round-trips serialised through SQLite's single-writer lock.
///
/// `ScanDbBatcher` accumulates these records in memory and flushes them in
/// one SQLite transaction at each checkpoint interval (default: every 50
/// processed messages). This reduces DB overhead from O(N) transactions to
/// O(N/50) — a 50x reduction in write count, plus the per-transaction
/// overhead of acquiring a write lock and syncing WAL.
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
    pub fn new() -> Self {
        Self::default()
    }

    /// Buffer a sender-domain sighting to be flushed later.
    pub fn record_sighting(&mut self, domain: &str, tag: &str, is_rejection_candidate: bool) {
        self.sightings.push(PendingSighting {
            domain: domain.to_string(),
            tag: tag.to_string(),
            is_rejection_candidate,
        });
    }

    /// Buffer an audit-log rejection row to be flushed later.
    pub fn record_rejection(&mut self, message_id: &str, reason: &str) {
        self.rejections.push(PendingRejection {
            message_id: message_id.to_string(),
            reason: reason.to_string(),
        });
    }

    /// Buffer an ignored-noise record to be flushed later.
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

    /// Returns true if there are no buffered records pending.
    pub fn is_empty(&self) -> bool {
        self.sightings.is_empty() && self.rejections.is_empty() && self.ignored.is_empty()
    }

    /// Flush all buffered records to the DB in a single transaction.
    /// Best-effort: a DB error here must not abort the scan — these are
    /// bookkeeping records, not financial data.
    pub async fn flush(&mut self, pool: &Pool) -> Result<()> {
        if self.is_empty() {
            return Ok(());
        }

        let sightings = std::mem::take(&mut self.sightings);
        let rejections = std::mem::take(&mut self.rejections);
        let ignored = std::mem::take(&mut self.ignored);

        let conn = pool.get().await?;
        conn.interact(move |c| -> Result<()> {
            let tx = c.transaction().map_err(|e| anyhow::anyhow!("TX start: {}", e))?;

            // 1. Flush sightings — upsert so repeated scans over overlapping
            //    date ranges don't create duplicate rows.
            for s in &sightings {
                let _ = crate::db::sender_reputation::record_sighting(
                    &tx,
                    &s.domain,
                    &s.tag,
                );
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

            // 2. Flush audit-log rejections.
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

            // 3. Flush ignored-noise records.
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

            tx.commit().map_err(|e| anyhow::anyhow!("TX commit: {}", e))?;
            Ok(())
        })
        .await
        .map_err(|e| anyhow::anyhow!("Interact error: {}", e))??;

        Ok(())
    }
}
