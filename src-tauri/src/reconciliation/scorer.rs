//! Scores candidate matches when the answer is not exact.
//!
//! Weighs amount, timing, merchant and instrument agreement into a confidence
//! score. The thresholds are deliberately conservative: scoring decides between
//! merging automatically and raising a cluster for the user, and the cost of a
//! wrong automatic merge is far higher than the cost of an unnecessary question.
use crate::reconciliation::engine::{CanonicalCandidate, IncomingObservation};
use chrono::NaiveDateTime;
use strsim::jaro_winkler;

pub const MERCHANT_SIMILARITY_WEIGHT: f64 = 0.30;
pub const TIME_PROXIMITY_WEIGHT_MAX: f64 = 0.25;
pub const REFERENCE_ID_MATCH_WEIGHT: f64 = 0.25;
pub const REFERENCE_ID_PARTIAL_WEIGHT: f64 = 0.10;
pub const REFERENCE_ID_MISMATCH_PENALTY: f64 = -0.25;
pub const AMOUNT_EXACTNESS_WEIGHT: f64 = 0.10;
pub const DIRECTION_CONSISTENCY_WEIGHT: f64 = 0.05;
pub const CROSS_SOURCE_COMPLEMENTARITY_BONUS: f64 = 0.05;

#[derive(Debug, Clone)]
pub struct ScoredCandidate {
    pub candidate_id: String,
    pub score: f64,
}

/// Scores candidates on amount, timing, merchant and instrument agreement.
pub fn score_candidates(
    obs: &IncomingObservation,
    candidates: &[CanonicalCandidate],
) -> Vec<ScoredCandidate> {
    let mut scored: Vec<ScoredCandidate> = candidates
        .iter()
        .map(|c| {
            let mut score: f64 = 0.0;

            if let (Some(obs_merchant), Some(cand_merchant)) =
                (&obs.merchant_raw, &c.merchant_normalized_name)
            {
                let obs_lower = obs_merchant.to_lowercase();
                let cand_lower = cand_merchant.to_lowercase();
                let similarity = jaro_winkler(&obs_lower, &cand_lower);
                if similarity > 0.0 {
                    score += MERCHANT_SIMILARITY_WEIGHT * similarity;
                }
            }

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

            if obs.amount_minor == c.amount_minor {
                score += AMOUNT_EXACTNESS_WEIGHT;
            }

            if obs.direction == c.direction {
                score += DIRECTION_CONSISTENCY_WEIGHT;
            }

            let time_score = compute_time_proximity_score(&obs.event_time, &c.event_time);
            score += time_score;

            if is_cross_source_complementary(&obs.source_pipeline, c.source_mix.as_deref()) {
                score += CROSS_SOURCE_COMPLEMENTARITY_BONUS;
            }

            ScoredCandidate {
                candidate_id: c.id.clone(),
                score: score.clamp(0.0, 1.0),
            }
        })
        .collect();

    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored
}

/// Whether two observations come from complementary sources.
///
/// An email alert and a statement line describing one payment is the expected
/// pattern, so cross-source pairs are weighted differently from two observations
/// of the same kind -- which are more likely genuinely separate transactions.
fn is_cross_source_complementary(
    obs_source_pipeline: &str,
    candidate_source_mix: Option<&str>,
) -> bool {
    matches!(
        (obs_source_pipeline, candidate_source_mix),
        ("statement_pdf", Some("email_only")) | ("gmail_transaction", Some("statement_only"))
    )
}

/// Scores how close two timestamps are.
///
/// Time is the weakest signal, because an authorisation alert and a statement
/// posting for one payment can be days apart.
fn compute_time_proximity_score(obs_time_str: &str, cand_time_str: &str) -> f64 {
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
            return 0.15 + (0.10 * (1.0 - (delta_seconds / 86400.0)));
        } else if delta_seconds <= 3.0 * 86400.0 {
            return 0.10 * (1.0 - ((delta_seconds - 86400.0) / (2.0 * 86400.0)));
        }
        return 0.0;
    }

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
        let score = compute_time_proximity_score("2026-06-10 14:32:00", "2026-06-10 20:15:00");
        assert!((0.15..=0.25).contains(&score));

        let score2 = compute_time_proximity_score("2026-06-10 14:32:00", "2026-06-12 14:32:00");
        assert!(score2 > 0.0 && score2 <= 0.10);

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
            confidence_score: None,
            event_time_confidence: None,
            channel: None,
        }
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

    #[test]
    fn test_scoring_exact_amount_time_merchant_high_score() {
        let scored = score_candidates(&base_obs(), &[base_candidate()]);
        assert_eq!(scored.len(), 1);
        assert!(scored[0].score > 0.70);
    }

    #[test]
    fn test_jaro_winkler_merchant_similarity() {
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

        assert!(score_mismatch < score_absent);
    }

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
        assert!(
            (scored_with_bonus[0].score
                - scored_without_bonus[0].score
                - CROSS_SOURCE_COMPLEMENTARITY_BONUS)
                .abs()
                < 1e-9
        );
    }

    #[test]
    fn test_semantic_dedup_daily_digest_vs_realtime_alert() {
        let mut obs_digest = base_obs();
        obs_digest.id = "obs_digest".to_string();
        obs_digest.amount_minor = 1500;
        obs_digest.event_time = "2026-06-11 08:00:00".to_string();
        obs_digest.reference_id = None;
        obs_digest.merchant_raw = Some("Target".to_string());
        obs_digest.source_record_id = "msg2".to_string();

        let mut cand_realtime = base_candidate();
        cand_realtime.id = "tx_realtime".to_string();
        cand_realtime.amount_minor = 1500;
        cand_realtime.event_time = "2026-06-10 18:30:00".to_string();
        cand_realtime.reference_id = None;
        cand_realtime.merchant_normalized_name = Some("Target".to_string());

        let scored = score_candidates(&obs_digest, &[cand_realtime]);
        assert_eq!(scored.len(), 1);

        assert!(scored[0].score >= 0.60);
    }
}
