use crate::reconciliation::engine::{CanonicalCandidate, IncomingObservation};
use chrono::NaiveDateTime;
use strsim::levenshtein;

/// A candidate with its computed score.
#[derive(Debug, Clone)]
pub struct ScoredCandidate {
    pub candidate_id: String,
    pub score: f64,
}

/// Scores all candidates for an incoming observation using multiple signals.
/// Returns candidates sorted by score descending.
///
/// Scoring signals (per Doc 11 §4):
///  - Merchant name similarity     (0–0.30)
///  - Time delta proximity         (0–0.25)  — uses source timestamps only, not wall clock
///  - Reference ID overlap         (0–0.25)
///  - Amount exactness             (0–0.10)  — always exact within candidate window
///  - Direction consistency        (0–0.05)  — always consistent within candidate window
///  - Statement provenance bonus   (+0.05)   — if observation came from a statement
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
                    score += 0.30;
                } else {
                    let dist = levenshtein(&obs_lower, &cand_lower) as f64;
                    let max_len = obs_lower.len().max(cand_lower.len()) as f64;
                    if max_len > 0.0 {
                        let similarity = 1.0 - (dist / max_len);
                        if similarity > 0.0 {
                            score += 0.30 * similarity;
                        }
                    }
                }
            }

            // Reference ID overlap
            if let (Some(obs_ref), Some(cand_ref)) = (&obs.reference_id, &c.reference_id) {
                if obs_ref == cand_ref {
                    score += 0.25;
                } else if obs_ref.contains(cand_ref.as_str()) || cand_ref.contains(obs_ref.as_str())
                {
                    score += 0.10;
                }
            }

            // Amount exactness (guaranteed by candidate window, give full points)
            if obs.amount_minor == c.amount_minor {
                score += 0.10;
            }

            // Direction consistency (guaranteed by candidate window, give full points)
            if obs.direction == c.direction {
                score += 0.05;
            }

            // Time delta proximity (using chrono parsing)
            let time_score = compute_time_proximity_score(&obs.event_time, &c.event_time);
            score += time_score;

            // Statement provenance bonus
            if obs.source_pipeline == "statement" {
                score += 0.05;
            }

            ScoredCandidate {
                candidate_id: c.id.clone(),
                score: score.min(1.0),
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

    #[test]
    fn test_score_candidates() {
        let obs = IncomingObservation {
            id: "obs1".to_string(),
            instrument_id: "inst1".to_string(),
            amount_minor: 1000,
            currency: "USD".to_string(),
            direction: "debit".to_string(),
            event_time: "2026-06-10 14:32:00".to_string(),
            reference_id: Some("REF123".to_string()),
            merchant_raw: Some("Uber".to_string()),
            source_pipeline: "gmail".to_string(),
            source_record_id: "msg1".to_string(),
            emi_total_installments: None,
            emi_original_amount_minor: None,
        };

        let cand = CanonicalCandidate {
            id: "cand1".to_string(),
            instrument_id: "inst1".to_string(),
            amount_minor: 1000,
            currency: "USD".to_string(),
            direction: "debit".to_string(),
            event_time: "2026-06-10 15:32:00".to_string(),
            reference_id: Some("REF123".to_string()),
            merchant_normalized_name: Some("Uber".to_string()),
            source_mix: None,
        };

        let scored = score_candidates(&obs, &[cand]);
        assert_eq!(scored.len(), 1);
        assert!(scored[0].score > 0.70); // High confidence due to exact match on amount, ref, time, merchant
    }

    #[test]
    fn test_levenshtein_distance_calculation() {
        let obs = IncomingObservation {
            id: "obs1".to_string(),
            instrument_id: "inst1".to_string(),
            amount_minor: 1000,
            currency: "USD".to_string(),
            direction: "debit".to_string(),
            event_time: "2026-06-10 14:32:00".to_string(),
            reference_id: None,
            merchant_raw: Some("AMZN Mktp US".to_string()),
            source_pipeline: "gmail".to_string(),
            source_record_id: "msg1".to_string(),
            emi_total_installments: None,
            emi_original_amount_minor: None,
        };

        let cand1 = CanonicalCandidate {
            id: "cand1".to_string(),
            instrument_id: "inst1".to_string(),
            amount_minor: 1000,
            currency: "USD".to_string(),
            direction: "debit".to_string(),
            event_time: "2026-06-10 14:32:00".to_string(),
            reference_id: None,
            merchant_normalized_name: Some("Amazon Marketplace".to_string()),
            source_mix: None,
        };

        let cand2 = CanonicalCandidate {
            id: "cand2".to_string(),
            instrument_id: "inst1".to_string(),
            amount_minor: 1000,
            currency: "USD".to_string(),
            direction: "debit".to_string(),
            event_time: "2026-06-10 14:32:00".to_string(),
            reference_id: None,
            merchant_normalized_name: Some("amzn mktp us".to_string()),
            source_mix: None,
        };

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

    #[test]
    fn test_scoring_engine_weights() {
        let obs = IncomingObservation {
            id: "obs1".to_string(),
            instrument_id: "inst1".to_string(),
            amount_minor: 500,
            currency: "USD".to_string(),
            direction: "debit".to_string(),
            event_time: "2026-06-10 14:32:00".to_string(),
            reference_id: Some("REF-123".to_string()),
            merchant_raw: Some("Spotify".to_string()),
            source_pipeline: "statement".to_string(), // gets +0.05
            source_record_id: "msg1".to_string(),
            emi_total_installments: None,
            emi_original_amount_minor: None,
        };

        let cand = CanonicalCandidate {
            id: "cand1".to_string(),
            instrument_id: "inst1".to_string(),
            amount_minor: 500, // +0.10
            currency: "USD".to_string(),
            direction: "debit".to_string(),                // +0.05
            event_time: "2026-06-10 14:32:00".to_string(), // +0.25
            reference_id: Some("REF-123".to_string()),     // +0.25
            merchant_normalized_name: Some("Spotify".to_string()), // +0.30
            source_mix: None,
        };

        let scored = score_candidates(&obs, &[cand]);
        assert_eq!(scored.len(), 1);

        // Expected score: 0.10 + 0.05 + 0.25 + 0.25 + 0.30 + 0.05 = 1.00
        assert_eq!(scored[0].score, 1.0);
    }

    #[test]
    fn test_semantic_dedup_daily_digest_vs_realtime_alert() {
        let obs_digest = IncomingObservation {
            id: "obs_digest".to_string(),
            instrument_id: "inst1".to_string(),
            amount_minor: 1500,
            currency: "USD".to_string(),
            direction: "debit".to_string(),
            event_time: "2026-06-11 08:00:00".to_string(), // Digest arrives next morning
            reference_id: None,
            merchant_raw: Some("Target".to_string()),
            source_pipeline: "gmail".to_string(),
            source_record_id: "msg2".to_string(),
            emi_total_installments: None,
            emi_original_amount_minor: None,
        };

        let cand_realtime = CanonicalCandidate {
            id: "tx_realtime".to_string(),
            instrument_id: "inst1".to_string(),
            amount_minor: 1500,
            currency: "USD".to_string(),
            direction: "debit".to_string(),
            event_time: "2026-06-10 18:30:00".to_string(), // Realtime was previous evening
            reference_id: None,
            merchant_normalized_name: Some("Target".to_string()),
            source_mix: None,
        };

        let scored = score_candidates(&obs_digest, &[cand_realtime]);
        assert_eq!(scored.len(), 1);

        // They should match since they are within 1 day (time proximity score will be > 0.15)
        // Merchant is exact (+0.30), Amount is exact (+0.10), Direction is exact (+0.05)
        // Total score > 0.60
        assert!(scored[0].score >= 0.60);
    }
}
