use crate::reconciliation::audit::{append_match_decision, DecisionType};
use crate::reconciliation::canonical::create_canonical_transaction;
use crate::reconciliation::cluster::create_ambiguity_cluster;
use crate::reconciliation::scorer::{score_candidates, ScoredCandidate};
use anyhow::Result;
use rusqlite::Connection;

use serde::{Deserialize, Serialize};

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

    Ok(rows
        .into_iter()
        .map(|r| CanonicalCandidate {
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
        })
        .collect())
}

/// Main reconciliation entry point: takes an observation and a set of candidate canonical
/// transactions fetched from the DB for that instrument/amount/direction/window.
/// Returns the decision type taken.
pub fn reconcile(
    conn: &Connection,
    obs: &IncomingObservation,
    candidates: Vec<CanonicalCandidate>,
) -> Result<DecisionType> {
    // ── Stage 1: Exact deterministic match ──────────────────────────────────
    let exact_matches: Vec<&CanonicalCandidate> = candidates
        .iter()
        .filter(|c| {
            c.instrument_id == obs.instrument_id
                && c.amount_minor == obs.amount_minor
                && c.currency == obs.currency
                && c.direction == obs.direction
                && obs.reference_id.is_some()
                && c.reference_id == obs.reference_id
        })
        .collect();

    if exact_matches.len() == 1 {
        let matched = exact_matches[0];

        // Doc 30 TASK-TXN-010: statement-overrides-email precedence rule,
        // plus transaction_observations.canonical_transaction_id linking --
        // applies to every matched decision, not just the statement case
        // (C3 fix, below, only ever handled the statement-arrives half of
        // the precedence rule and never linked the observation at all).
        crate::reconciliation::canonical::apply_match_precedence_and_link(
            conn,
            obs,
            &matched.id,
            matched.source_mix.as_deref(),
        )?;

        append_match_decision(
            conn,
            &obs.id,
            Some(&matched.id),
            1.0,
            DecisionType::AutoMatchedExact,
        )?;
        tracing::info!(
            observation_id = obs.id,
            canonical_id = matched.id,
            decision = "AutoMatchedExact",
            score = 1.0,
            "Reconciliation decision completed"
        );
        return Ok(DecisionType::AutoMatchedExact);
    }

    if exact_matches.len() > 1 {
        // Multiple exact matches → ambiguous
        let ids: Vec<String> = exact_matches.iter().map(|c| c.id.clone()).collect();
        let cluster_id = create_ambiguity_cluster(conn, &obs.id, &ids)?;
        append_match_decision(
            conn,
            &obs.id,
            None,
            0.0,
            DecisionType::AmbiguousPending(cluster_id.clone()),
        )?;
        tracing::info!(
            observation_id = obs.id,
            cluster_id = cluster_id,
            decision = "AmbiguousPending",
            reason = "multiple_exact_matches",
            "Reconciliation decision completed"
        );
        return Ok(DecisionType::AmbiguousPending(cluster_id));
    }

    // ── Stage 2: Scored candidate matching ──────────────────────────────────
    if candidates.is_empty() {
        // No candidates at all → new canonical transaction
        create_canonical_transaction(conn, obs)?;
        append_match_decision(conn, &obs.id, None, 0.0, DecisionType::NewCanonical)?;
        tracing::info!(
            observation_id = obs.id,
            decision = "NewCanonical",
            reason = "no_candidates",
            "Reconciliation decision completed"
        );
        return Ok(DecisionType::NewCanonical);
    }

    let scored = score_candidates(obs, &candidates);

    // Base viability threshold: 0.55
    let viable: Vec<&ScoredCandidate> = scored.iter().filter(|s| s.score >= 0.55).collect();

    if viable.is_empty() {
        // No viable match → new canonical
        create_canonical_transaction(conn, obs)?;
        append_match_decision(conn, &obs.id, None, 0.0, DecisionType::NewCanonical)?;
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

    // Relative-margin model: >15% margin between top and runner-up → clear winner
    if (top.score - second.score) > 0.15 {
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
    let cluster_id = create_ambiguity_cluster(conn, &obs.id, &ids)?;
    append_match_decision(
        conn,
        &obs.id,
        None,
        top.score,
        DecisionType::AmbiguousPending(cluster_id.clone()),
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
