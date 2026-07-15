//! Doc 30 TASK-DEDUP-004: Multi-Signal Confidence Scoring Engine.
//!
//! Composite score from weighted signals: amount, time proximity, merchant
//! similarity, reference-ID match, and source-pipeline complementarity.
//! Weights are versioned, documented constants — not magic numbers — so
//! scoring changes are trackable/benchmarkable (Doc 30's own requirement).

use crate::reconciliation::engine::{CanonicalCandidate, IncomingObservation};
use chrono::NaiveDateTime;
use strsim::levenshtein;

/// Merchant name similarity (Jaro-Winkler-style via Levenshtein ratio),
/// full weight on an exact case-insensitive match.
pub const MERCHANT_SIMILARITY_WEIGHT: f64 = 0.30;
/// Time delta proximity — smooth decay within the ±3-day candidate window,
/// not a hard cutoff (Doc 30 TASK-DEDUP-004). Ceiling of the range returned
/// by `compute_time_proximity_score`.
pub const TIME_PROXIMITY_WEIGHT_MAX: f64 = 0.25;
/// Reference-ID exact match — a strong boost.
pub const REFERENCE_ID_MATCH_WEIGHT: f64 = 0.25;
/// Reference-ID partial/substring overlap — a weaker boost than an exact match.
pub const REFERENCE_ID_PARTIAL_WEIGHT: f64 = 0.10;
/// Reference-ID mismatch when both sides have one present — a strong
/// penalty (Doc 30: "strong penalty on mismatch when both present"). Two
/// different, unrelated reference IDs is positive evidence the candidate is
/// the *wrong* transaction, not merely neutral.
pub const REFERENCE_ID_MISMATCH_PENALTY: f64 = -0.25;
/// Amount exactness — always exact within the candidate window (Doc 30
/// TASK-DEDUP-003 never fuzzy-matches amount), so this is really a
/// same-currency confirmation weight rather than a variance-tolerant one.
pub const AMOUNT_EXACTNESS_WEIGHT: f64 = 0.10;
/// Direction consistency — always consistent within the candidate window,
/// same rationale as amount exactness above.
pub const DIRECTION_CONSISTENCY_WEIGHT: f64 = 0.05;
/// Source-pipeline complementarity bonus: an email-sourced candidate matched
/// against an incoming statement observation (or vice versa) is the
/// *expected* legitimate corroboration case, not a coincidental duplicate
/// (Doc 30 TASK-DEDUP-004).
pub const CROSS_SOURCE_COMPLEMENTARITY_BONUS: f64 = 0.05;

/// A candidate with its computed score.
#[derive(Debug, Clone)]
pub struct ScoredCandidate {
    pub candidate_id: String,
    pub score: f64,
}

/// Scores all candidates for an incoming observation using multiple signals.
/// Returns candidates sorted by score descending.
pub fn score_candidates(
    obs: &IncomingObservation,
    candidates: &[CanonicalCandidate],
) -> Vec<ScoredCandidate> {
    let mut scored: Vec<ScoredCandidate> = candidates
        .iter()
        .map(|c| {
            let mut score: f64 = 0.0;

            // Merchant name similarity using Levenshtein distance
            if let (Some(obs_merchant), Some(cand_merchant)) =
                (&obs.merchant_raw, &c.merchant_normalized_name)
            {
                let obs_lower = obs_merchant.to_lowercase();
                let cand_lower = cand_merchant.to_lowercase();
                if obs_lower == cand_lower {
                    score += MERCHANT_SIMILARITY_WEIGHT;
                } else {
                    let dist = levenshtein(&obs_lower, &cand_lower) as f64;
                    let max_len = obs_lower.len().max(cand_lower.len()) as f64;
                    if max_len > 0.0 {
                        let similarity = 1.0 - (dist / max_len);
                        if similarity > 0.0 {
                            score += MERCHANT_SIMILARITY_WEIGHT * similarity;
                        }
                    }
                }
            }

            // Reference ID overlap: boost on match, strong penalty on
            // mismatch when both are present (Doc 30 TASK-DEDUP-004).
            if let (Some(obs_ref), Some(cand_ref)) = (&obs.reference_id, &c.reference_id) {
                if obs_ref == cand_ref {
                    score += REFERENCE_ID_MATCH_WEIGHT;
                } else if obs_ref.contains(cand_ref.as_str()) || cand_ref.contains(obs_ref.as_str())
                {
                    score += REFERENCE_ID_PARTIAL_WEIGHT;
                } else {
                    score += REFERENCE_ID_MISMATCH_PENALTY;
                }
            }

            // Amount exactness (guaranteed by candidate window, give full points)
            if obs.amount_minor == c.amount_minor {
                score += AMOUNT_EXACTNESS_WEIGHT;
            }

            // Direction consistency (guaranteed by candidate window, give full points)
            if obs.direction == c.direction {
                score += DIRECTION_CONSISTENCY_WEIGHT;
            }

            // Time delta proximity (using chrono parsing)
            let time_score = compute_time_proximity_score(&obs.event_time, &c.event_time);
            score += time_score;

            // Source-pipeline complementarity bonus
            if is_cross_source_complementary(&obs.source_pipeline, c.source_mix.as_deref()) {
                score += CROSS_SOURCE_COMPLEMENTARITY_BONUS;
            }

            ScoredCandidate {
                candidate_id: c.id.clone(),
                score: score.clamp(0.0, 1.0),
            }
        })
        .collect();

    // Sort descending by score
    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored
}

/// Doc 30 TASK-DEDUP-004: "an email-sourced candidate matched against an
/// incoming statement observation, or vice versa, is treated as the
/// *expected* legitimate corroboration case." Uses `source_mix` (the
/// canonical candidate's actual evidence composition) rather than the
/// candidate's own `source_pipeline` — canonical transactions don't carry
/// one, `source_mix` is the field Doc 18 §4.3 gives for exactly this
/// purpose.
fn is_cross_source_complementary(obs_source_pipeline: &str, candidate_source_mix: Option<&str>) -> bool {
    matches!(
        (obs_source_pipeline, candidate_source_mix),
        ("statement_pdf", Some("email_only")) | ("gmail_transaction", Some("statement_only"))
    )
}

/// Computes a time proximity score (0–0.25) based on how close two ISO-8601 UTC timestamps are.
/// This must use source event timestamps — never ingestion time or local wall-clock.
fn compute_time_proximity_score(obs_time_str: &str, cand_time_str: &str) -> f64 {
    // Attempt to parse as NaiveDateTime, fallback to simple date comparison if parsing fails
    let fmt = "%Y-%m-%d %H:%M:%S";
    if let (Ok(obs_dt), Ok(cand_dt)) = (
        NaiveDateTime::parse_from_str(obs_time_str, fmt)
            .or_else(|_| NaiveDateTime::parse_from_str(&format!("{} 00:00:00", obs_time_str), fmt)),
        NaiveDateTime::parse_from_str(cand_time_str, fmt).or_else(|_| {
            NaiveDateTime::parse_from_str(&format!("{} 00:00:00", cand_time_str), fmt)
        }),
    ) {
        let delta_seconds =
            (obs_dt.and_utc().timestamp() - cand_dt.and_utc().timestamp()).abs() as f64;

        if delta_seconds <= 86400.0 {
            // Within 1 day (0-24 hrs) -> map to 0.15 - 0.25
            return 0.15 + (0.10 * (1.0 - (delta_seconds / 86400.0)));
        } else if delta_seconds <= 3.0 * 86400.0 {
            // Within 3 days -> map to 0.0 - 0.10
            return 0.10 * (1.0 - ((delta_seconds - 86400.0) / (2.0 * 86400.0)));
        }
        return 0.0;
    }

    // Fallback: If both timestamps start with the same date (YYYY-MM-DD), award full proximity.
    if obs_time_str.len() >= 10
        && cand_time_str.len() >= 10
        && obs_time_str[..10] == cand_time_str[..10]
    {
        return 0.25;
    }
    if obs_time_str.len() >= 7
        && cand_time_str.len() >= 7
        && obs_time_str[..7] == cand_time_str[..7]
    {
        return 0.10;
    }
    0.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reconciliation::engine::{CanonicalCandidate, IncomingObservation};

    #[test]
    fn test_compute_time_proximity_score() {
        // Same day
        let score = compute_time_proximity_score("2026-06-10 14:32:00", "2026-06-10 20:15:00");
        assert!(score >= 0.15 && score <= 0.25);

        // Within 3 days
        let score2 = compute_time_proximity_score("2026-06-10 14:32:00", "2026-06-12 14:32:00");
        assert!(score2 > 0.0 && score2 <= 0.10);

        // Over 3 days
        let score3 = compute_time_proximity_score("2026-06-10 14:32:00", "2026-06-15 14:32:00");
        assert_eq!(score3, 0.0);
    }

    fn base_obs() -> IncomingObservation {
        IncomingObservation {
            id: "obs1".to_string(),
            instrument_id: "inst1".to_string(),
            amount_minor: 1000,
            currency: "USD".to_string(),
            direction: "debit".to_string(),
            event_time: "2026-06-10 14:32:00".to_string(),
            reference_id: Some("REF123".to_string()),
            merchant_raw: Some("Uber".to_string()),
            source_pipeline: "gmail_transaction".to_string(),
            source_record_id: "msg1".to_string(),
            emi_total_installments: None,
            emi_original_amount_minor: None,
            fingerprint: None,
            confidence_score: None,        }
    }

    fn base_candidate() -> CanonicalCandidate {
        CanonicalCandidate {
            id: "cand1".to_string(),
            instrument_id: "inst1".to_string(),
            amount_minor: 1000,
            currency: "USD".to_string(),
            direction: "debit".to_string(),
            event_time: "2026-06-10 15:32:00".to_string(),
            reference_id: Some("REF123".to_string()),
            merchant_normalized_name: Some("Uber".to_string()),
            source_mix: None,
        }
    }

    /// Doc 30 TASK-DEDUP-004 acceptance test.
    #[test]
    fn test_scoring_exact_amount_time_merchant_high_score() {
        let scored = score_candidates(&base_obs(), &[base_candidate()]);
        assert_eq!(scored.len(), 1);
        assert!(scored[0].score > 0.70); // High confidence due to exact match on amount, ref, time, merchant
    }

    #[test]
    fn test_levenshtein_distance_calculation() {
        let mut obs = base_obs();
        obs.merchant_raw = Some("AMZN Mktp US".to_string());
        obs.reference_id = None;

        let mut cand1 = base_candidate();
        cand1.id = "cand1".to_string();
        cand1.event_time = "2026-06-10 14:32:00".to_string();
        cand1.reference_id = None;
        cand1.merchant_normalized_name = Some("Amazon Marketplace".to_string());

        let mut cand2 = base_candidate();
        cand2.id = "cand2".to_string();
        cand2.event_time = "2026-06-10 14:32:00".to_string();
        cand2.reference_id = None;
        cand2.merchant_normalized_name = Some("amzn mktp us".to_string());

        let scored = score_candidates(&obs, &[cand1, cand2]);
        assert_eq!(scored.len(), 2);

        // cand2 has exact match (case insensitive) -> should have higher score than cand1 which is similar but different len
        let score_cand2 = scored
            .iter()
            .find(|s| s.candidate_id == "cand2")
            .unwrap()
            .score;
        let score_cand1 = scored
            .iter()
            .find(|s| s.candidate_id == "cand1")
            .unwrap()
            .score;

        assert!(score_cand2 > score_cand1);
    }

    /// Doc 30 TASK-DEDUP-004 acceptance test: weights are named, documented
    /// constants — verified here by summing them directly rather than
    /// hardcoding the expected total, so a future weight change can't
    /// silently desync this test from the real values.
    #[test]
    fn test_scoring_weights_are_versioned_and_documented() {
        let mut obs = base_obs();
        obs.amount_minor = 500;
        obs.currency = "USD".to_string();
        obs.reference_id = Some("REF-123".to_string());
        obs.merchant_raw = Some("Spotify".to_string());
        obs.source_pipeline = "statement_pdf".to_string();

        let mut cand = base_candidate();
        cand.amount_minor = 500;
        cand.event_time = obs.event_time.clone();
        cand.reference_id = Some("REF-123".to_string());
        cand.merchant_normalized_name = Some("Spotify".to_string());
        cand.source_mix = Some("email_only".to_string());

        let scored = score_candidates(&obs, &[cand]);
        assert_eq!(scored.len(), 1);

        let expected_total = (AMOUNT_EXACTNESS_WEIGHT
            + DIRECTION_CONSISTENCY_WEIGHT
            + REFERENCE_ID_MATCH_WEIGHT
            + TIME_PROXIMITY_WEIGHT_MAX
            + MERCHANT_SIMILARITY_WEIGHT
            + CROSS_SOURCE_COMPLEMENTARITY_BONUS)
            .min(1.0);
        assert_eq!(scored[0].score, expected_total);
        assert_eq!(scored[0].score, 1.0);
    }

    /// Doc 30 TASK-DEDUP-004 acceptance test: two different, both-present
    /// reference IDs must heavily penalize the score, not merely skip the
    /// boost — positive evidence this is the *wrong* candidate.
    #[test]
    fn test_scoring_ref_id_mismatch_heavily_penalizes() {
        let mut obs = base_obs();
        obs.reference_id = Some("REF_AAA".to_string());

        let mut cand_mismatch = base_candidate();
        cand_mismatch.reference_id = Some("REF_ZZZ_UNRELATED".to_string());

        let mut cand_absent = base_candidate();
        cand_absent.id = "cand_absent".to_string();
        cand_absent.reference_id = None;

        let scored = score_candidates(&obs, &[cand_mismatch, cand_absent]);
        let score_mismatch = scored
            .iter()
            .find(|s| s.candidate_id == "cand1")
            .unwrap()
            .score;
        let score_absent = scored
            .iter()
            .find(|s| s.candidate_id == "cand_absent")
            .unwrap()
            .score;

        // A candidate with a *different* reference ID must score strictly
        // lower than an otherwise-identical candidate with no reference ID
        // at all (neutral, not penalized).
        assert!(score_mismatch < score_absent);
    }

    /// Doc 30 TASK-DEDUP-004 acceptance test: an email-sourced observation
    /// matched against a statement-sourced candidate (or vice versa) gets
    /// the cross-source complementarity bonus — the expected corroboration
    /// case, not a coincidental duplicate.
    #[test]
    fn test_scoring_cross_source_complementarity_boost() {
        let mut obs_statement = base_obs();
        obs_statement.source_pipeline = "statement_pdf".to_string();

        let mut cand_email_sourced = base_candidate();
        cand_email_sourced.source_mix = Some("email_only".to_string());

        let mut cand_no_provenance = base_candidate();
        cand_no_provenance.id = "cand_no_provenance".to_string();
        cand_no_provenance.source_mix = None;

        let scored_with_bonus = score_candidates(&obs_statement, &[cand_email_sourced]);
        let scored_without_bonus = score_candidates(&obs_statement, &[cand_no_provenance]);

        assert!(scored_with_bonus[0].score > scored_without_bonus[0].score);
        assert!((scored_with_bonus[0].score - scored_without_bonus[0].score - CROSS_SOURCE_COMPLEMENTARITY_BONUS).abs() < 1e-9);
    }

    #[test]
    fn test_semantic_dedup_daily_digest_vs_realtime_alert() {
        let mut obs_digest = base_obs();
        obs_digest.id = "obs_digest".to_string();
        obs_digest.amount_minor = 1500;
        obs_digest.event_time = "2026-06-11 08:00:00".to_string(); // Digest arrives next morning
        obs_digest.reference_id = None;
        obs_digest.merchant_raw = Some("Target".to_string());
        obs_digest.source_record_id = "msg2".to_string();

        let mut cand_realtime = base_candidate();
        cand_realtime.id = "tx_realtime".to_string();
        cand_realtime.amount_minor = 1500;
        cand_realtime.event_time = "2026-06-10 18:30:00".to_string(); // Realtime was previous evening
        cand_realtime.reference_id = None;
        cand_realtime.merchant_normalized_name = Some("Target".to_string());

        let scored = score_candidates(&obs_digest, &[cand_realtime]);
        assert_eq!(scored.len(), 1);

        // They should match since they are within 1 day (time proximity score will be > 0.15)
        // Merchant is exact (+0.30), Amount is exact (+0.10), Direction is exact (+0.05)
        // Total score > 0.60
        assert!(scored[0].score >= 0.60);
    }
}
