use crate::reconciliation::audit::{append_match_decision, DecisionType};
use crate::reconciliation::canonical::create_canonical_transaction;
use crate::reconciliation::cluster::create_ambiguity_cluster;
use crate::reconciliation::scorer::{score_candidates, ScoredCandidate};
use anyhow::Result;
use rusqlite::Connection;

use serde::{Deserialize, Serialize};

/// Doc 30 TASK-DEDUP-005 / Document 12 §8.2's Relative Margin Model: a
/// candidate must clear this score before being considered viable at all.
/// Anything below it, or zero viable candidates, routes to `new_canonical`.
pub const BASE_VIABILITY_FLOOR: f64 = 0.55;
/// Doc 30 TASK-DEDUP-005 / Document 12 §8.2: the margin the top-scoring
/// viable candidate must beat the runner-up by to be a "Clear Winner"
/// (`auto_matched_scored`). Candidates within this margin of each other are
/// routed to `ambiguous_pending` instead of force-picking the highest —
/// "ambiguous matches must be kept unresolved rather than forced."
pub const AMBIGUITY_MARGIN_THRESHOLD: f64 = 0.15;

/// Represents a normalized observation coming from Gmail or a statement PDF.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncomingObservation {
    pub id: String,
    pub instrument_id: String,
    pub amount_minor: i64,
    pub currency: String,
    pub direction: String,  // "debit" | "credit"
    pub event_time: String, // UTC ISO-8601 from source (email Date header or statement date)
    pub reference_id: Option<String>,
    pub merchant_raw: Option<String>,
    pub source_pipeline: String, // "gmail_transaction" | "statement_pdf" | "manual"
    pub source_record_id: String,
    /// Doc 30 TASK-TXN-012: EMI language detected during extraction, if any.
    pub emi_total_installments: Option<i32>,
    pub emi_original_amount_minor: Option<i64>,
    /// Doc 30 TASK-TXN-008 / TASK-DEDUP-001: the observation's pre-computed
    /// SHA-256 fingerprint (`transaction_observations.fingerprint`), consumed
    /// by the fingerprint pre-filter in `reconcile()` below before the more
    /// expensive windowed candidate generation / scoring engine ever run.
    /// `None` when the caller couldn't compute one (e.g. manual entries have
    /// no `connected_account_id` to hash) -- the pre-filter is simply
    /// skipped in that case, falling straight through to windowed search.
    #[serde(default)]
    pub fingerprint: Option<String>,
}

/// Represents an existing canonical transaction for candidate matching.
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
    /// Doc 18 §4.3 (`email_only`/`statement_only`/`merged`/`manual`) --
    /// needed to decide which side of the statement-overrides-email
    /// precedence rule (Doc 30 TASK-TXN-010) applies to a match.
    pub source_mix: Option<String>,
}

pub fn fetch_candidates(
    conn: &Connection,
    obs: &IncomingObservation,
) -> Result<Vec<CanonicalCandidate>> {
    let fmt = "%Y-%m-%d %H:%M:%S";
    let event_time_dt = chrono::NaiveDateTime::parse_from_str(&obs.event_time, fmt)
        .or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(&format!("{} 00:00:00", obs.event_time), fmt)
        })
        .unwrap_or_default();

    let rows = crate::db::transactions::find_candidates_within_window(
        conn,
        &obs.instrument_id,
        obs.amount_minor,
        &obs.direction,
        &event_time_dt,
        3,
    )?;

    Ok(rows.into_iter().map(to_canonical_candidate).collect())
}

/// Shared row->candidate mapping used both by the windowed search above
/// (TASK-DEDUP-003) and by the fingerprint pre-filter's single-row lookup
/// (TASK-DEDUP-001) in `reconcile()` below.
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

/// Main reconciliation entry point: takes an observation and a set of candidate canonical
/// transactions fetched from the DB for that instrument/amount/direction/window.
/// Returns the decision type taken.
pub fn reconcile(
    conn: &Connection,
    obs: &IncomingObservation,
    candidates: Vec<CanonicalCandidate>,
) -> Result<DecisionType> {
    // ── Stage 0: Fingerprint pre-filter (Doc 30 TASK-DEDUP-001) ──────────────
    // An indexed lookup via idx_transaction_observations_fingerprint, tried
    // before the windowed candidate set (already fetched by the caller via
    // `fetch_candidates`, TASK-DEDUP-003) is scored at all. An exact
    // fingerprint match against an already-reconciled observation is an
    // extremely strong exact-match candidate — verified against the strict
    // condition (TASK-DEDUP-002) and, if it holds, resolved without ever
    // touching the fuzzy scoring engine below.
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
                    tracing::info!(
                        observation_id = obs.id,
                        canonical_id = candidate.id,
                        decision = "AutoMatchedExact",
                        score = 1.0,
                        reason = "fingerprint_prefilter",
                        "Reconciliation decision completed"
                    );
                    return Ok(DecisionType::AutoMatchedExact);
                }
                // Doc 30 TASK-DEDUP-002: a rare fingerprint collision (hash
                // matches, strict fields don't) falls through to full
                // scoring rather than assuming a match — never routed to
                // ambiguous on the strength of a hash collision alone.
                tracing::debug!(
                    observation_id = obs.id,
                    canonical_id = candidate.id,
                    "Fingerprint pre-filter hit but exact-match conditions failed \
                     (rare collision) — falling through to full scoring"
                );
            }
        }
    }

    // ── Stage 1: Scored candidate matching ──────────────────────────────────
    if candidates.is_empty() {
        // No candidates at all → new canonical transaction
        create_canonical_transaction(conn, obs)?;
        append_match_decision(conn, &obs.id, None, 0.0, DecisionType::NewCanonical, None)?;
        tracing::info!(
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
        // No viable match → new canonical
        create_canonical_transaction(conn, obs)?;
        append_match_decision(conn, &obs.id, None, 0.0, DecisionType::NewCanonical, None)?;
        tracing::info!(
            observation_id = obs.id,
            decision = "NewCanonical",
            reason = "no_viable_candidates",
            "Reconciliation decision completed"
        );
        return Ok(DecisionType::NewCanonical);
    }

    let top = viable[0];

    if viable.len() == 1 {
        // Single viable candidate → auto_matched_scored
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
        tracing::info!(
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

    // Relative-margin model: margin exceeding the ambiguity threshold between
    // top and runner-up → clear winner.
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
        tracing::info!(
            observation_id = obs.id,
            canonical_id = top.candidate_id,
            decision = "AutoMatchedScored",
            score = top.score,
            reason = "margin_exceeded",
            "Reconciliation decision completed"
        );
        return Ok(DecisionType::AutoMatchedScored);
    }

    // Within 15% margin → ambiguous
    let ids: Vec<String> = viable.iter().map(|s| s.candidate_id.clone()).collect();
    let cluster_id = create_ambiguity_cluster(
        conn,
        &obs.id,
        &obs.instrument_id,
        obs.amount_minor,
        &obs.direction,
        &obs.event_time,
        top.score,
        &ids,
    )?;
    append_match_decision(
        conn,
        &obs.id,
        None,
        top.score,
        DecisionType::AmbiguousPending(cluster_id.clone()),
        None,
    )?;
    tracing::info!(
        observation_id = obs.id,
        cluster_id = cluster_id,
        decision = "AmbiguousPending",
        score = top.score,
        reason = "margin_too_close",
        "Reconciliation decision completed"
    );
    Ok(DecisionType::AmbiguousPending(cluster_id))
}
