//! The matching engine: fetch candidates, score them, act on the verdict.
//!
//! Two entry points, both owning their transaction so matching and the writes
//! that follow commit together (audit A-9): `ingest_observation` for a new
//! observation, `complete_observation` for one held in review whose missing
//! fields have arrived (Layer 6). Human cluster resolution is the only other
//! canonical writer (`cluster::resolve_cluster`).
use crate::reconciliation::audit::{append_match_decision, DecisionType};
use crate::reconciliation::canonical::create_canonical_transaction;
use crate::reconciliation::cluster::create_ambiguity_cluster;
use crate::reconciliation::scorer::{score_candidates, ScoredCandidate};
use anyhow::Result;
use rusqlite::Connection;

use serde::{Deserialize, Serialize};
use crate::db::transaction_observations::TransactionObservationsRow;

pub const BASE_VIABILITY_FLOOR: f64 = 0.55;
pub const AMBIGUITY_MARGIN_THRESHOLD: f64 = 0.15;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncomingObservation {
    pub id: String,
    pub instrument_id: String,
    pub amount_minor: i64,
    pub currency: String,
    pub direction: String,
    pub event_time: String,
    pub reference_id: Option<String>,
    pub merchant_raw: Option<String>,
    pub source_pipeline: String,
    pub source_record_id: String,
    pub emi_total_installments: Option<i32>,
    pub emi_original_amount_minor: Option<i64>,
    #[serde(default)]
    pub fingerprint: Option<String>,
    #[serde(default)]
    pub confidence_score: Option<f64>,
    #[serde(default)]
    pub event_time_confidence: Option<String>,
    #[serde(default)]
    pub channel: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CanonicalCandidate {
    pub id: String,
    pub instrument_id: String,
    pub amount_minor: i64,
    pub currency: String,
    pub direction: String,
    pub event_time: String,
    pub reference_id: Option<String>,
    pub merchant_normalized_name: Option<String>,
    pub source_mix: Option<String>,
}

/// Fetches plausible match candidates for an observation.
pub fn fetch_candidates(
    conn: &Connection,
    obs: &IncomingObservation,
) -> Result<Vec<CanonicalCandidate>> {
    let fmt = "%Y-%m-%d %H:%M:%S";
    let event_time_dt = chrono::NaiveDateTime::parse_from_str(&obs.event_time, fmt)
        .or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(&format!("{} 00:00:00", obs.event_time), fmt)
        })
        .map_err(|e| {
            anyhow::anyhow!(
                "Observation {} has an unparseable event_time {:?} ({}). Refusing to \
                 reconcile: searching a candidate window around the Unix epoch would \
                 create a duplicate canonical transaction instead of matching the \
                 existing one.",
                obs.id,
                obs.event_time,
                e
            )
        })?;

    let rows = crate::db::transactions::find_candidates_within_window(
        conn,
        &obs.instrument_id,
        obs.amount_minor,
        &obs.currency,
        &obs.direction,
        &event_time_dt,
        3,
    )?;

    // Hard constraint ahead of scoring (review P1): a canonical that already
    // holds an observation from this pipeline is a separate payment unless the
    // reference ids agree, however close the score would be.
    let mut candidates = Vec::with_capacity(rows.len());
    for row in rows {
        if !conflicts_with_same_source(conn, &row.id, obs)? {
            candidates.push(to_canonical_candidate(row));
        }
    }
    Ok(candidates)
}

fn conflicts_with_same_source(conn: &Connection, canonical_id: &str, obs: &IncomingObservation) -> Result<bool> {
    crate::db::transaction_observations::canonical_has_conflicting_same_source(
        conn,
        canonical_id,
        &obs.source_pipeline,
        obs.reference_id.as_deref(),
        &obs.id,
    )
}

/// Projects a transaction row into a match candidate.
fn to_canonical_candidate(r: crate::db::transactions::TransactionsRow) -> CanonicalCandidate {
    CanonicalCandidate {
        id: r.id,
        instrument_id: r.instrument_id.unwrap_or_default(),
        amount_minor: r.amount_minor.unwrap_or(0),
        currency: r.currency.unwrap_or_default(),
        direction: r.direction.unwrap_or_default(),
        event_time: r
            .best_event_time
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_default(),
        reference_id: r.reference_id,
        merchant_normalized_name: r.merchant_normalized_name,
        source_mix: r.source_mix,
    }
}

/// The single shared reconciliation entry point.
///
/// Ensures the insertion of the observation and its canonicalization 
/// occur atomically in the same transaction. A crash midway cannot leave 
/// an observation half-merged.
/// Opens BEGIN IMMEDIATE, or a savepoint when a transaction is already open.
fn begin_scope(conn: &Connection) -> rusqlite::Result<bool> {
    if conn.is_autocommit() {
        conn.execute_batch("BEGIN IMMEDIATE")?;
        Ok(false)
    } else {
        conn.execute_batch("SAVEPOINT ingest_observation")?;
        Ok(true)
    }
}

fn commit_scope(conn: &Connection, nested: bool) -> rusqlite::Result<()> {
    conn.execute_batch(if nested { "RELEASE ingest_observation" } else { "COMMIT" })
}

fn rollback_scope(conn: &Connection, nested: bool) -> rusqlite::Result<()> {
    conn.execute_batch(if nested {
        "ROLLBACK TO ingest_observation; RELEASE ingest_observation"
    } else {
        "ROLLBACK"
    })
}

pub fn ingest_observation(
    conn: &Connection,
    row: &TransactionObservationsRow,
    unresolved_reason: &str,
) -> Result<(crate::db::transaction_observations::InsertObservationOutcome, Option<DecisionType>)> {
    // Inside a caller's transaction (a statement commit) this runs as a
    // savepoint, so the caller can make many observations all-or-nothing
    // (review H7); on its own it opens BEGIN IMMEDIATE as before.
    let nested = begin_scope(conn)?;
    
    let insert_outcome = crate::db::transaction_observations::insert_observation_idempotent(conn, row);
    
    match insert_outcome {
        Ok(crate::db::transaction_observations::InsertObservationOutcome::Inserted) => {
            // Guard (audit A-2): an observation with no instrument cannot be
            // reconciled, so it never lands without a review entry; otherwise
            // it is stored but shown nowhere. `unresolved_reason` says why.
            if row.instrument_id.is_none() {
                let routed = crate::db::unassigned_transactions::insert(
                    conn,
                    &crate::db::unassigned_transactions::UnassignedTransactionRow {
                        id: uuid::Uuid::new_v4().to_string(),
                        observation_id: row.id.clone(),
                        reason: unresolved_reason.to_string(),
                        status: "open".to_string(),
                        created_at: None,
                    },
                );
                if let Err(e) = routed {
                    let _ = rollback_scope(conn, nested);
                    return Err(e);
                }
                commit_scope(conn, nested)?;
                return Ok((crate::db::transaction_observations::InsertObservationOutcome::Inserted, None));
            }
            
            let incoming = IncomingObservation {
                id: row.id.clone(),
                instrument_id: row.instrument_id.clone().unwrap(),
                amount_minor: row.amount_minor.unwrap_or(0),
                currency: row.currency.clone().unwrap_or_else(|| "INR".to_string()),
                direction: row.direction.clone().unwrap_or_else(|| "debit".to_string()),
                event_time: row
                    .event_time
                    .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                    .unwrap_or_default(),
                reference_id: row.reference_id.clone(),
                merchant_raw: row.merchant_raw.clone(),
                source_pipeline: row.source_pipeline.clone().unwrap_or_default(),
                source_record_id: row.source_record_id.clone().unwrap_or_default(),
                emi_total_installments: row.emi_total_installments,
                emi_original_amount_minor: row.emi_original_amount_minor,
                fingerprint: row.fingerprint.clone(),
                confidence_score: row.confidence_score,
                event_time_confidence: row.event_time_confidence.clone(),
                channel: row.channel.clone(),
            };
            
            // Reconciliation runs inside a savepoint. If it fails -- most
            // often a missing or unparseable event time breaking the candidate
            // lookup -- only its writes are undone; the observation is kept and
            // sent to the review queue instead of the whole insert being
            // rolled back and the email silently lost (review M6).
            conn.execute_batch("SAVEPOINT reconcile")?;
            let reconciled = if incoming.event_time.is_empty() {
                Err(anyhow::anyhow!("observation has no event time"))
            } else {
                fetch_candidates(conn, &incoming)
                    .and_then(|candidates| reconcile(conn, &incoming, candidates))
            };
            let decision = match reconciled {
                Ok(decision) => {
                    conn.execute_batch("RELEASE reconcile")?;
                    Some(decision)
                }
                Err(e) => {
                    tracing::warn!("Reconciliation failed for obs '{}', sending to review: {:#}", row.id, e);
                    let reason = if incoming.event_time.is_empty() {
                        "missing_event_time"
                    } else {
                        "reconciliation_failed"
                    };
                    let routed = conn
                        .execute_batch("ROLLBACK TO reconcile; RELEASE reconcile")
                        .map_err(anyhow::Error::from)
                        .and_then(|_| {
                            crate::db::unassigned_transactions::insert(
                                conn,
                                &crate::db::unassigned_transactions::UnassignedTransactionRow {
                                    id: uuid::Uuid::new_v4().to_string(),
                                    observation_id: row.id.clone(),
                                    reason: reason.to_string(),
                                    status: "open".to_string(),
                                    created_at: None,
                                },
                            )
                        });
                    if let Err(e) = routed {
                        let _ = rollback_scope(conn, nested);
                        return Err(e);
                    }
                    None
                }
            };
            if let Err(e) = commit_scope(conn, nested) {
                let _ = rollback_scope(conn, nested);
                return Err(e.into());
            }
            Ok((crate::db::transaction_observations::InsertObservationOutcome::Inserted, decision))
        }
        Ok(crate::db::transaction_observations::InsertObservationOutcome::DuplicateSkipped) => {
            commit_scope(conn, nested)?;
            Ok((crate::db::transaction_observations::InsertObservationOutcome::DuplicateSkipped, None))
        }
        Err(e) => {
            let _ = rollback_scope(conn, nested);
            Err(e)
        }
    }
}

/// Completes an observation held in review once its missing fields have been
/// filled in (Layer 6). The observation update, the reconciliation and the
/// settlement of the review entry commit as one transaction, so a crash cannot
/// leave a booked transaction beside a still-open review item (audit A-9).
///
/// With `review_reason` set, the observation is updated and stays in review
/// under that reason. Returns the reconciliation decision, or None when it
/// stays in review or the user already settled the entry.
pub fn complete_observation(
    conn: &Connection,
    row: &TransactionObservationsRow,
    unassigned_id: &str,
    review_reason: Option<&str>,
) -> Result<Option<DecisionType>> {
    let nested = begin_scope(conn)?;
    let result = (|| -> Result<Option<DecisionType>> {
        // Checked inside the write transaction: the user may have resolved or
        // dismissed the entry while the enrichment ran.
        if !crate::db::unassigned_transactions::is_open(conn, unassigned_id)? {
            return Ok(None);
        }
        crate::db::transaction_observations::update_observation(conn, row)?;
        if let Some(reason) = review_reason {
            crate::db::unassigned_transactions::update_reason(conn, unassigned_id, reason)?;
            return Ok(None);
        }
        let instrument_id = row
            .instrument_id
            .clone()
            .ok_or_else(|| anyhow::anyhow!("observation '{}' has no instrument", row.id))?;
        let incoming = IncomingObservation {
            id: row.id.clone(),
            instrument_id,
            amount_minor: row.amount_minor.unwrap_or(0),
            currency: row.currency.clone().unwrap_or_else(|| "INR".to_string()),
            direction: row.direction.clone().unwrap_or_else(|| "debit".to_string()),
            event_time: row
                .event_time
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_default(),
            reference_id: row.reference_id.clone(),
            merchant_raw: row.merchant_raw.clone(),
            source_pipeline: row
                .source_pipeline
                .clone()
                .unwrap_or_else(|| "gmail_transaction".to_string()),
            source_record_id: row.source_record_id.clone().unwrap_or_default(),
            emi_total_installments: row.emi_total_installments,
            emi_original_amount_minor: row.emi_original_amount_minor,
            fingerprint: row.fingerprint.clone(),
            confidence_score: row.confidence_score,
            event_time_confidence: row.event_time_confidence.clone(),
            channel: row.channel.clone(),
        };
        let decision =
            fetch_candidates(conn, &incoming).and_then(|candidates| reconcile(conn, &incoming, candidates))?;
        crate::db::unassigned_transactions::settle_if_open(conn, unassigned_id, "resolved")?;
        Ok(Some(decision))
    })();
    match result {
        Ok(decision) => {
            commit_scope(conn, nested)?;
            Ok(decision)
        }
        Err(e) => {
            let _ = rollback_scope(conn, nested);
            Err(e)
        }
    }
}

/// Reconciles an observation, merging it or raising a cluster.
pub fn reconcile(
    conn: &Connection,
    obs: &IncomingObservation,
    candidates: Vec<CanonicalCandidate>,
) -> Result<DecisionType> {
    if let Some(fp) = obs.fingerprint.as_deref() {
        if let Some(canonical_id) =
            crate::reconciliation::prefilter::fingerprint_prefilter_lookup(conn, fp, &obs.id)?
        {
            let row = if conflicts_with_same_source(conn, &canonical_id, obs)? {
                None
            } else {
                crate::db::transactions::get_transaction(conn, &canonical_id)?
            };
            if let Some(row) = row {
                let candidate = to_canonical_candidate(row);
                if crate::reconciliation::exact_match::verify_exact_match(obs, &candidate) {
                    crate::reconciliation::canonical::apply_match_precedence_and_link(
                        conn,
                        obs,
                        &candidate.id,
                        candidate.source_mix.as_deref(),
                    )?;
                    append_match_decision(
                        conn,
                        &obs.id,
                        Some(&candidate.id),
                        1.0,
                        DecisionType::AutoMatchedExact,
                        None,
                    )?;
                    tracing::debug!(
                        observation_id = obs.id,
                        canonical_id = candidate.id,
                        decision = "AutoMatchedExact",
                        score = 1.0,
                        reason = "fingerprint_prefilter",
                        "Reconciliation decision completed"
                    );
                    return Ok(DecisionType::AutoMatchedExact);
                }
                tracing::debug!(
                    observation_id = obs.id,
                    canonical_id = candidate.id,
                    "Fingerprint pre-filter hit but exact-match conditions failed \
                     (rare collision) — falling through to full scoring"
                );
            }
        }
    }

    if candidates.is_empty() {
        create_canonical_transaction(conn, obs)?;
        append_match_decision(conn, &obs.id, None, 0.0, DecisionType::NewCanonical, None)?;
        tracing::debug!(
            observation_id = obs.id,
            decision = "NewCanonical",
            reason = "no_candidates",
            "Reconciliation decision completed"
        );
        return Ok(DecisionType::NewCanonical);
    }

    let scored = score_candidates(obs, &candidates);

    let viable: Vec<&ScoredCandidate> = scored
        .iter()
        .filter(|s| s.score >= BASE_VIABILITY_FLOOR)
        .collect();

    if viable.is_empty() {
        create_canonical_transaction(conn, obs)?;
        append_match_decision(conn, &obs.id, None, 0.0, DecisionType::NewCanonical, None)?;
        tracing::debug!(
            observation_id = obs.id,
            decision = "NewCanonical",
            reason = "no_viable_candidates",
            "Reconciliation decision completed"
        );
        return Ok(DecisionType::NewCanonical);
    }

    let top = viable[0];

    if viable.len() == 1 {
        let matched_source_mix = candidates
            .iter()
            .find(|c| c.id == top.candidate_id)
            .and_then(|c| c.source_mix.clone());
        crate::reconciliation::canonical::apply_match_precedence_and_link(
            conn,
            obs,
            &top.candidate_id,
            matched_source_mix.as_deref(),
        )?;
        append_match_decision(
            conn,
            &obs.id,
            Some(&top.candidate_id),
            top.score,
            DecisionType::AutoMatchedScored,
            None,
        )?;
        tracing::debug!(
            observation_id = obs.id,
            canonical_id = top.candidate_id,
            decision = "AutoMatchedScored",
            score = top.score,
            reason = "single_viable_candidate",
            "Reconciliation decision completed"
        );
        return Ok(DecisionType::AutoMatchedScored);
    }

    let second = viable[1];

    if (top.score - second.score) > AMBIGUITY_MARGIN_THRESHOLD {
        let matched_source_mix = candidates
            .iter()
            .find(|c| c.id == top.candidate_id)
            .and_then(|c| c.source_mix.clone());
        crate::reconciliation::canonical::apply_match_precedence_and_link(
            conn,
            obs,
            &top.candidate_id,
            matched_source_mix.as_deref(),
        )?;
        append_match_decision(
            conn,
            &obs.id,
            Some(&top.candidate_id),
            top.score,
            DecisionType::AutoMatchedScored,
            None,
        )?;
        tracing::debug!(
            observation_id = obs.id,
            canonical_id = top.candidate_id,
            decision = "AutoMatchedScored",
            score = top.score,
            reason = "margin_exceeded",
            "Reconciliation decision completed"
        );
        return Ok(DecisionType::AutoMatchedScored);
    }

    let scored_candidates: Vec<ScoredCandidate> = viable.iter().map(|s| (*s).clone()).collect();
    let cluster_id = create_ambiguity_cluster(
        conn,
        &obs.id,
        &obs.instrument_id,
        obs.amount_minor,
        &obs.direction,
        &obs.event_time,
        top.score,
        &scored_candidates,
    )?;
    append_match_decision(
        conn,
        &obs.id,
        None,
        top.score,
        DecisionType::AmbiguousPending(cluster_id.clone()),
        None,
    )?;
    tracing::debug!(
        observation_id = obs.id,
        cluster_id = cluster_id,
        decision = "AmbiguousPending",
        score = top.score,
        reason = "margin_too_close",
        "Reconciliation decision completed"
    );
    Ok(DecisionType::AmbiguousPending(cluster_id))
}
