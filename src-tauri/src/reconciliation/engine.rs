//! The matching engine: fetch candidates, score them, act on the verdict.
//!
//! `reconcile_transactionally` is the entry point that matters. Matching and the
//! write that follows happen in one transaction, so a crash midway cannot leave
//! an observation half-merged into a canonical record.
use crate::reconciliation::audit::{append_match_decision, DecisionType};
use crate::reconciliation::canonical::create_canonical_transaction;
use crate::reconciliation::cluster::create_ambiguity_cluster;
use crate::reconciliation::scorer::{score_candidates, ScoredCandidate};
use anyhow::Result;
use rusqlite::Connection;

use serde::{Deserialize, Serialize};

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

    Ok(rows.into_iter().map(to_canonical_candidate).collect())
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

/// Reconciles within a transaction, so matching and the resulting write commit together.
///
/// Atomicity matters here: a crash between deciding a match and writing it would
/// leave an observation half-merged into a canonical record.
pub fn reconcile_transactionally(
    conn: &Connection,
    obs: &IncomingObservation,
) -> Result<DecisionType> {
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result =
        fetch_candidates(conn, obs).and_then(|candidates| reconcile(conn, obs, candidates));
    match result {
        Ok(decision) => {
            conn.execute_batch("COMMIT")?;
            Ok(decision)
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
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
            if let Some(row) = crate::db::transactions::get_transaction(conn, &canonical_id)? {
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
