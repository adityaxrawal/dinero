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
    /// Doc 30 TASK-DEDUP-008: this observation's own extraction confidence
    /// (`transaction_observations.confidence_score`, Document 18 §4.4),
    /// consumed by the email-vs-email precedence path in
    /// `canonical::apply_match_precedence_and_link` — "first-arriving data
    /// generally retained unless the second has materially higher-confidence
    /// fields." `None` when unavailable (e.g. manual entries).
    #[serde(default)]
    pub confidence_score: Option<f64>,
    /// The observation's `transaction_observations.event_time_confidence`
    /// (set by `extraction::ladder::apply_date_cross_check` when the body
    /// date was numerically ambiguous, `None` otherwise). Consumed by
    /// `canonical::create_canonical_transaction` instead of unconditionally
    /// stamping the canonical row `"high"` -- see that function for why an
    /// ambiguous-date observation must not silently look fully trusted.
    #[serde(default)]
    pub event_time_confidence: Option<String>,
    /// Display-only transaction rail/channel (`extraction::ladder::detect_channel`).
    /// Pure metadata pass-through to the canonical `transactions` row --
    /// never read by matching/scoring in this module.
    #[serde(default)]
    pub channel: Option<String>,
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
    // audit_02 #4: this used to be `.unwrap_or_default()`, which silently
    // yielded `NaiveDateTime::default()` -- the Unix epoch. The candidate
    // window below is +/-3 days around this value, so an unparseable
    // `event_time` searched 1970 instead of the transaction's actual date,
    // reliably found nothing, and let the caller create a *new* canonical
    // transaction for an event that already had one. That is a silent
    // duplicate in the user's financial history, produced by a date bug two
    // layers upstream.
    //
    // The upstream producers (`ingestion::queues`, `reconciliation::cluster`)
    // format a real `NaiveDateTime` -- always parseable -- but fall back to
    // `unwrap_or_default()` (the empty string) when the stored `event_time`
    // is NULL. So the realistic input here is `""`, not a malformed date.
    // Failing closed surfaces that as a rolled-back reconciliation
    // (`reconcile_transactionally` rolls back on `Err`) rather than a
    // duplicate row nobody notices.
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

/// Doc 30 TASK-DEDUP-003/005/006: every production call site fetches
/// candidates and reconciles as two separate, un-transacted statements. In
/// WAL mode with multiple pooled connections (the Transaction Queue's 4
/// workers, plus statement-import and manual-entry call sites, each on
/// their own connection), two callers racing to reconcile the same
/// real-world transaction (e.g. an email alert and a statement row for the
/// same purchase) can both `fetch_candidates` and see "no match" before
/// either commits, then both independently create a canonical transaction
/// or ambiguity cluster -- a real duplicate. `BEGIN IMMEDIATE` takes
/// SQLite's write lock up front rather than deferring it to the first
/// write, so the second caller's `BEGIN IMMEDIATE` blocks (via `PRAGMA
/// busy_timeout`, set once per pooled connection in `db::init_db`) until
/// the first caller's transaction commits -- at which point its own
/// `fetch_candidates` sees the just-committed row and matches against it
/// instead of creating a duplicate. All production callers should route
/// through this function rather than calling `fetch_candidates`/`reconcile`
/// directly.
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

/// Main reconciliation entry point: takes an observation and a set of candidate canonical
/// transactions fetched from the DB for that instrument/amount/direction/window.
/// Returns the decision type taken.
/// audit_09 #3: every branch below logs its outcome at `debug!`, not `info!`.
/// Exactly one fires per reconciliation, so at `info!` a 100k-message
/// historical scan wrote 100k lines that drowned out every genuine warning in
/// the same file — the finding's own example of a level policy that makes log
/// filtering impractical.
///
/// Nothing is lost by dropping the level: `append_match_decision` writes the
/// same observation id, canonical id, decision and score to `match_decisions`
/// in this very function, authoritatively and queryably, and that table (not
/// the log) is what `do_get_debug_metrics` and the support bundle read.
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
        // No viable match → new canonical
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

    // Within 15% margin → ambiguous
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
