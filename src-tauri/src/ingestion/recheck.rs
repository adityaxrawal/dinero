//! Re-checking a review entry on demand (current-issues #3).
//!
//! Reruns the rule layers, learned patterns first, on the email stored with
//! the entry; what they complete is booked through the same path the LLM
//! uses, and what they cannot is handed to the local AI. Every write checks
//! inside its transaction that the entry is still open and has no live AI
//! job, so a double click, a manual save or a dismiss racing a re-check
//! cannot book twice.

use crate::ingestion::queues::Layer6Job;
use deadpool_sqlite::Pool;
use rusqlite::OptionalExtension;
use serde::Serialize;
use tokio::sync::mpsc;

/// What a re-check did with one entry.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecheckOutcome {
    /// The rule layers completed it and it was booked.
    Booked,
    /// Handed to the local AI; hidden from the queue until its verdict.
    QueuedForAi,
    /// The rules found nothing bookable and no model can run here; the
    /// entry stays, with anything they found filled in.
    NoAiAvailable,
    /// An AI job for this entry is already running.
    AlreadyQueued,
}

/// Totals for "Re-check all".
#[derive(Debug, Default, Serialize, PartialEq, Eq)]
pub struct RecheckSummary {
    pub booked: usize,
    pub queued_for_ai: usize,
    pub no_ai_available: usize,
    pub skipped: usize,
}

/// What the AI side of a re-check needs from the running app.
pub struct AiHandle<'a> {
    /// Whether a model can run on this device now.
    pub ready: bool,
    pub app_dir: Option<&'a std::path::Path>,
    pub tx: &'a mpsc::Sender<Layer6Job>,
}

/// The entry's state and stored email, read in one go.
struct Entry {
    observation_id: String,
    bank: String,
    body: String,
    event_time_seconds: Option<i64>,
}

/// Why an entry cannot be re-checked, worded for the user.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RecheckError {
    #[error("This entry was already resolved or dismissed.")]
    NotOpen,
    #[error("This entry has no stored email to re-check (it came from a statement or the reconciliation engine).")]
    NoEmail,
    #[error("{0}")]
    Internal(String),
}

impl From<anyhow::Error> for RecheckError {
    fn from(e: anyhow::Error) -> Self {
        Self::Internal(format!("{e:#}"))
    }
}

const LIVE_JOB: &str =
    "EXISTS (SELECT 1 FROM layer6_pending_jobs j WHERE j.id = u.id AND j.dead_at IS NULL)";

/// Re-checks one review entry.
pub async fn recheck_entry(
    pool: &Pool,
    ai: &AiHandle<'_>,
    unassigned_id: &str,
) -> Result<RecheckOutcome, RecheckError> {
    let entry = match load_entry(pool, unassigned_id).await? {
        Loaded::Entry(e) => e,
        Loaded::Queued => return Ok(RecheckOutcome::AlreadyQueued),
    };

    // Learned patterns, then templates, regex and NLP, exactly as the scan
    // runs them (no inline LLM).
    let mut trace = crate::logging::EmailTrace::new(unassigned_id);
    let extracted = crate::extraction::ladder::run_extraction_ladder(
        pool,
        &entry.bank,
        &entry.body,
        ai.app_dir.map(std::path::Path::to_path_buf),
        false,
        entry.event_time_seconds,
        &mut false,
        None,
        &mut trace,
    )
    .await?;
    if let Some(mut obs) = extracted {
        crate::ingestion::message_processor::MessageProcessor::clean_merchant(&mut obs, &entry.body);
        // Books only if complete, confident and from a trusted sender;
        // otherwise it fills the entry's gaps and keeps it in review.
        let booked = crate::ingestion::queues::apply_layer6_success(
            pool,
            &entry.observation_id,
            unassigned_id,
            obs,
            entry.event_time_seconds,
        )
        .await?;
        if booked {
            return Ok(RecheckOutcome::Booked);
        }
    }

    if !ai.ready {
        return Ok(RecheckOutcome::NoAiAvailable);
    }
    queue_for_ai(pool, ai, unassigned_id, entry).await
}

/// Re-checks every eligible entry in the queue, one at a time.
pub async fn recheck_all(pool: &Pool, ai: &AiHandle<'_>) -> Result<RecheckSummary, RecheckError> {
    let ids = pool
        .get()
        .await
        .map_err(|e| RecheckError::Internal(e.to_string()))?
        .interact(|c| crate::db::unassigned_transactions::select_open_with_context(c))
        .await
        .map_err(|e| RecheckError::Internal(e.to_string()))??
        .into_iter()
        .filter(|d| d.can_recheck)
        .map(|d| d.id)
        .collect::<Vec<_>>();

    let mut summary = RecheckSummary::default();
    for id in ids {
        match recheck_entry(pool, ai, &id).await {
            Ok(RecheckOutcome::Booked) => summary.booked += 1,
            Ok(RecheckOutcome::QueuedForAi) => summary.queued_for_ai += 1,
            Ok(RecheckOutcome::NoAiAvailable) => summary.no_ai_available += 1,
            // Resolved, dismissed or already queued meanwhile: nothing to do.
            Ok(RecheckOutcome::AlreadyQueued) | Err(RecheckError::NotOpen | RecheckError::NoEmail) => {
                summary.skipped += 1
            }
            Err(e @ RecheckError::Internal(_)) => {
                tracing::warn!("Re-check failed for unassigned_id='{}': {}", id, e);
                summary.skipped += 1;
            }
        }
    }
    Ok(summary)
}

/// status, live AI job, observation id, raw payload, event time.
type EntryRow = (String, bool, String, Option<String>, Option<chrono::NaiveDateTime>);

enum Loaded {
    Entry(Entry),
    Queued,
}

async fn load_entry(pool: &Pool, unassigned_id: &str) -> Result<Loaded, RecheckError> {
    let id = unassigned_id.to_string();
    let conn = pool.get().await.map_err(|e| RecheckError::Internal(e.to_string()))?;
    conn.interact(move |c| -> Result<Loaded, RecheckError> {
        let row: Option<EntryRow> = c
            .query_row(
                &format!(
                    "SELECT u.status, {LIVE_JOB}, u.observation_id, o.raw_payload_json, o.event_time
                     FROM unassigned_transactions u
                     JOIN transaction_observations o ON o.id = u.observation_id
                     WHERE u.id = ?1"
                ),
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()
            .map_err(|e| RecheckError::Internal(e.to_string()))?;
        let Some((status, live_job, observation_id, raw, event_time)) = row else {
            return Err(RecheckError::NotOpen);
        };
        if status != "open" {
            return Err(RecheckError::NotOpen);
        }
        if live_job {
            return Ok(Loaded::Queued);
        }
        let payload: serde_json::Value = raw
            .as_deref()
            .and_then(|r| serde_json::from_str(r).ok())
            .ok_or(RecheckError::NoEmail)?;
        let (Some(bank), Some(body)) = (payload["bank"].as_str(), payload["body"].as_str()) else {
            return Err(RecheckError::NoEmail);
        };
        Ok(Loaded::Entry(Entry {
            observation_id,
            bank: bank.to_string(),
            body: body.to_string(),
            event_time_seconds: event_time.and_then(ist_naive_to_unix),
        }))
    })
    .await
    .map_err(|e| RecheckError::Internal(e.to_string()))?
}

/// Observation times are stored as IST wall-clock time (see
/// `apply_layer6_success`); the ladder wants a Unix timestamp.
fn ist_naive_to_unix(dt: chrono::NaiveDateTime) -> Option<i64> {
    let ist = chrono::FixedOffset::east_opt(5 * 3600 + 30 * 60)?;
    dt.and_local_timezone(ist).single().map(|t| t.timestamp())
}

/// Writes the AI job and hands it to a worker.
///
/// The open/no-live-job check, the reason and the job row commit together,
/// so a racing re-check finds the job and a racing manual save finds the
/// entry claimed.
async fn queue_for_ai(
    pool: &Pool,
    ai: &AiHandle<'_>,
    unassigned_id: &str,
    entry: Entry,
) -> Result<RecheckOutcome, RecheckError> {
    let Some(app_dir) = ai.app_dir else {
        return Ok(RecheckOutcome::NoAiAvailable);
    };
    let job = Layer6Job {
        observation_id: entry.observation_id,
        unassigned_id: unassigned_id.to_string(),
        bank_name: entry.bank,
        body_text: entry.body,
        app_dir: app_dir.to_path_buf(),
        internal_date_seconds: entry.event_time_seconds,
    };
    let persisted = job.clone();
    let conn = pool.get().await.map_err(|e| RecheckError::Internal(e.to_string()))?;
    let outcome = conn
        .interact(move |c| -> Result<Option<RecheckOutcome>, RecheckError> {
            // IMMEDIATE: takes the write lock before the check, so two racing
            // re-checks cannot both see "no job" and both insert one.
            let tx = c
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                .map_err(|e| RecheckError::Internal(e.to_string()))?;
            let (status, live_job): (String, bool) = tx
                .query_row(
                    &format!("SELECT u.status, {LIVE_JOB} FROM unassigned_transactions u WHERE u.id = ?1"),
                    [&persisted.unassigned_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .map_err(|e| RecheckError::Internal(e.to_string()))?
                .ok_or(RecheckError::NotOpen)?;
            if status != "open" {
                return Err(RecheckError::NotOpen);
            }
            if live_job {
                return Ok(Some(RecheckOutcome::AlreadyQueued));
            }
            crate::db::unassigned_transactions::update_reason(
                &tx,
                &persisted.unassigned_id,
                "pending_llm_enrichment",
            )?;
            // A row left dead by the old attempt cap is replaced.
            tx.execute("DELETE FROM layer6_pending_jobs WHERE id = ?1", [&persisted.unassigned_id])
                .map_err(|e| RecheckError::Internal(e.to_string()))?;
            crate::db::layer6_jobs::insert(
                &tx,
                &crate::db::layer6_jobs::PendingLayer6Job {
                    id: persisted.unassigned_id.clone(),
                    observation_id: persisted.observation_id.clone(),
                    bank_name: persisted.bank_name.clone(),
                    body_text: persisted.body_text.clone(),
                    internal_date_seconds: persisted.internal_date_seconds,
                },
            )?;
            tx.commit().map_err(|e| RecheckError::Internal(e.to_string()))?;
            Ok(None)
        })
        .await
        .map_err(|e| RecheckError::Internal(e.to_string()))??;
    if let Some(early) = outcome {
        return Ok(early);
    }
    crate::ingestion::queues::offer_layer6_job(ai.tx, job);
    Ok(RecheckOutcome::QueuedForAi)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingestion::message_processor::{MessageProcessor, ReviewRoute};

    const COMPLETE: &str = "Rs. 2590.00 has been debited from your HDFC Bank Credit Card ending 0364 \
        towards RAZ*SWIGGY on 24 May, 2026 at 19:34:18 .";
    const NO_CARD: &str = "Rs. 2590.00 has been debited towards RAZ*SWIGGY on 24 May, 2026 at 19:34:18 .";

    struct Fixture {
        dir: std::path::PathBuf,
        pool: Pool,
        tx: mpsc::Sender<Layer6Job>,
        rx: mpsc::Receiver<Layer6Job>,
    }

    impl Fixture {
        async fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            let pool = crate::db::init_db(dir.join("t.db")).await.unwrap();
            let (tx, rx) = mpsc::channel(8);
            Self { dir, pool, tx, rx }
        }

        fn ai(&self, ready: bool) -> AiHandle<'_> {
            AiHandle { ready, app_dir: Some(&self.dir), tx: &self.tx }
        }

        /// Files an empty review entry holding `body`, as the scan would.
        async fn entry(&self, body: &str, route: ReviewRoute) -> String {
            let (_obs, id) = MessageProcessor::record_unassigned_transaction(
                &self.pool,
                &format!("msg-{}", uuid::Uuid::new_v4()),
                crate::extraction::ladder::ExtractionResult::default(),
                body,
                None,
                "gate3_failed:missing_instrument",
                Some("HDFC Bank"),
                route,
            )
            .await
            .unwrap()
            .unwrap();
            id
        }

        async fn status_reason(&self, id: &str) -> (String, String) {
            let id = id.to_string();
            self.pool
                .get()
                .await
                .unwrap()
                .interact(move |c| {
                    c.query_row(
                        "SELECT status, reason FROM unassigned_transactions WHERE id = ?1",
                        [id],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                })
                .await
                .unwrap()
                .unwrap()
        }

        async fn visible(&self) -> Vec<String> {
            self.pool
                .get()
                .await
                .unwrap()
                .interact(|c| crate::db::unassigned_transactions::select_open(c))
                .await
                .unwrap()
                .unwrap()
                .into_iter()
                .map(|r| r.id)
                .collect()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[tokio::test]
    async fn an_entry_the_rules_now_complete_is_booked() {
        let f = Fixture::new().await;
        let id = f.entry(COMPLETE, ReviewRoute::Review).await;
        let outcome = recheck_entry(&f.pool, &f.ai(true), &id).await.unwrap();
        assert_eq!(outcome, RecheckOutcome::Booked);
        assert_eq!(f.status_reason(&id).await.0, "resolved");
    }

    #[tokio::test]
    async fn an_incomplete_entry_goes_to_the_ai_hidden_and_a_second_click_is_refused() {
        let mut f = Fixture::new().await;
        let id = f.entry(NO_CARD, ReviewRoute::Review).await;
        assert_eq!(recheck_entry(&f.pool, &f.ai(true), &id).await.unwrap(), RecheckOutcome::QueuedForAi);
        assert_eq!(f.rx.try_recv().unwrap().unassigned_id, id);
        assert_eq!(f.status_reason(&id).await, ("open".into(), "pending_llm_enrichment".into()));
        assert!(!f.visible().await.contains(&id), "hidden while the AI reads it");

        assert_eq!(
            recheck_entry(&f.pool, &f.ai(true), &id).await.unwrap(),
            RecheckOutcome::AlreadyQueued,
            "a second click must not queue it twice"
        );
        assert!(f.rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn without_a_model_an_incomplete_entry_stays_in_review() {
        let f = Fixture::new().await;
        let id = f.entry(NO_CARD, ReviewRoute::Review).await;
        assert_eq!(recheck_entry(&f.pool, &f.ai(false), &id).await.unwrap(), RecheckOutcome::NoAiAvailable);
        assert!(f.visible().await.contains(&id));
    }

    /// Owner decision 2026-11-25 (E-3 / N-E1): a re-check pre-fills an
    /// untrusted sender's entry but never books it.
    #[tokio::test]
    async fn a_review_only_sender_is_prefilled_but_never_booked() {
        let f = Fixture::new().await;
        let id = f.entry(COMPLETE, ReviewRoute::ReviewOnlySender).await;
        assert_eq!(recheck_entry(&f.pool, &f.ai(false), &id).await.unwrap(), RecheckOutcome::NoAiAvailable);
        assert_eq!(f.status_reason(&id).await, ("open".into(), "unverified_sender".into()));
    }

    #[tokio::test]
    async fn a_resolved_entry_or_one_without_an_email_is_refused() {
        let f = Fixture::new().await;
        let id = f.entry(COMPLETE, ReviewRoute::Review).await;
        let conn = f.pool.get().await.unwrap();
        let claim_id = id.clone();
        assert!(conn
            .interact(move |c| crate::db::unassigned_transactions::claim_for_manual_resolve(c, &claim_id))
            .await
            .unwrap()
            .unwrap());
        assert_eq!(recheck_entry(&f.pool, &f.ai(true), &id).await, Err(RecheckError::NotOpen));

        let other = f.entry(COMPLETE, ReviewRoute::Review).await;
        let strip = other.clone();
        conn.interact(move |c| {
            c.execute(
                "UPDATE transaction_observations SET raw_payload_json = NULL
                 WHERE id = (SELECT observation_id FROM unassigned_transactions WHERE id = ?1)",
                [strip],
            )
        })
        .await
        .unwrap()
        .unwrap();
        assert_eq!(recheck_entry(&f.pool, &f.ai(true), &other).await, Err(RecheckError::NoEmail));
    }

    /// The race this feature opened: a manual save on a stale form while the
    /// AI reads (and may book) the same entry must be refused, not booked.
    #[tokio::test]
    async fn a_manual_save_cannot_claim_an_entry_the_ai_is_reading() {
        let f = Fixture::new().await;
        let id = f.entry(NO_CARD, ReviewRoute::Review).await;
        assert_eq!(recheck_entry(&f.pool, &f.ai(true), &id).await.unwrap(), RecheckOutcome::QueuedForAi);
        let conn = f.pool.get().await.unwrap();
        let claim_id = id.clone();
        let claimed = conn
            .interact(move |c| crate::db::unassigned_transactions::claim_for_manual_resolve(c, &claim_id))
            .await
            .unwrap()
            .unwrap();
        assert!(!claimed);
        assert_eq!(f.status_reason(&id).await.0, "open");
    }
}
