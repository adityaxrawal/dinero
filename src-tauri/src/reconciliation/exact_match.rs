//! Confirms an unambiguous match.
//!
//! When every distinguishing attribute agrees, no scoring is warranted -- this
//! resolves the common case directly and leaves genuine ambiguity to the scorer.
use crate::reconciliation::engine::{CanonicalCandidate, IncomingObservation};
use chrono::NaiveDateTime;

const EXACT_MATCH_TIME_TOLERANCE_SECONDS: i64 = 120;

/// Confirms every distinguishing attribute agrees.
pub fn verify_exact_match(obs: &IncomingObservation, candidate: &CanonicalCandidate) -> bool {
    obs.instrument_id == candidate.instrument_id
        && obs.direction == candidate.direction
        && obs.amount_minor == candidate.amount_minor
        && obs.currency == candidate.currency
        && time_within_tolerance(
            &obs.event_time,
            &candidate.event_time,
            EXACT_MATCH_TIME_TOLERANCE_SECONDS,
        )
}

/// Whether two timestamps fall within tolerance.
///
/// Tolerance is needed because the same payment is timestamped differently by an
/// authorisation alert and a statement posting.
fn time_within_tolerance(a: &str, b: &str, tolerance_seconds: i64) -> bool {
    let fmt = "%Y-%m-%d %H:%M:%S";
    let parse = |s: &str| {
        NaiveDateTime::parse_from_str(s, fmt)
            .or_else(|_| NaiveDateTime::parse_from_str(&format!("{} 00:00:00", s), fmt))
    };
    match (parse(a), parse(b)) {
        (Ok(dt_a), Ok(dt_b)) => {
            (dt_a.and_utc().timestamp() - dt_b.and_utc().timestamp()).abs() <= tolerance_seconds
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(
        instrument_id: &str,
        direction: &str,
        amount_minor: i64,
        event_time: &str,
    ) -> IncomingObservation {
        IncomingObservation {
            id: "obs_1".to_string(),
            instrument_id: instrument_id.to_string(),
            amount_minor,
            currency: "INR".to_string(),
            direction: direction.to_string(),
            event_time: event_time.to_string(),
            reference_id: None,
            merchant_raw: None,
            source_pipeline: "gmail_transaction".to_string(),
            source_record_id: "rec_1".to_string(),
            emi_total_installments: None,
            emi_original_amount_minor: None,
            fingerprint: Some("fp_1".to_string()),
            confidence_score: None,
            event_time_confidence: None,
            channel: None,
        }
    }

    fn candidate(
        instrument_id: &str,
        direction: &str,
        amount_minor: i64,
        event_time: &str,
    ) -> CanonicalCandidate {
        CanonicalCandidate {
            id: "cand_1".to_string(),
            instrument_id: instrument_id.to_string(),
            amount_minor,
            currency: "INR".to_string(),
            direction: direction.to_string(),
            event_time: event_time.to_string(),
            reference_id: None,
            merchant_normalized_name: None,
            source_mix: None,
        }
    }

    #[test]
    fn test_exact_match_rejects_currency_mismatch() {
        let o = obs("inst_1", "debit", 50000, "2026-06-10 14:00:00");
        let mut c = candidate("inst_1", "debit", 50000, "2026-06-10 14:00:30");
        assert!(
            verify_exact_match(&o, &c),
            "sanity: same-currency case must still match"
        );

        c.currency = "USD".to_string();
        assert!(
            !verify_exact_match(&o, &c),
            "a fingerprint collision across currencies must not auto-merge"
        );
    }

    #[test]
    fn test_exact_match_all_conditions_hold() {
        let o = obs("inst_1", "debit", 1000, "2026-06-10 14:00:00");
        let c = candidate("inst_1", "debit", 1000, "2026-06-10 14:01:30");
        assert!(verify_exact_match(&o, &c));
    }

    #[test]
    fn test_exact_match_fingerprint_collision_falls_through() {
        let o = obs("inst_1", "debit", 1000, "2026-06-10 14:00:00");

        assert!(!verify_exact_match(
            &o,
            &candidate("inst_1", "debit", 2000, "2026-06-10 14:00:30")
        ));
        assert!(!verify_exact_match(
            &o,
            &candidate("inst_2", "debit", 1000, "2026-06-10 14:00:30")
        ));
        assert!(!verify_exact_match(
            &o,
            &candidate("inst_1", "credit", 1000, "2026-06-10 14:00:30")
        ));
        assert!(!verify_exact_match(
            &o,
            &candidate("inst_1", "debit", 1000, "2026-06-10 14:05:00")
        ));
    }

    #[test]
    fn test_exact_match_time_boundary() {
        let o = obs("inst_1", "debit", 1000, "2026-06-10 14:00:00");
        assert!(verify_exact_match(
            &o,
            &candidate("inst_1", "debit", 1000, "2026-06-10 14:02:00")
        ));
        assert!(!verify_exact_match(
            &o,
            &candidate("inst_1", "debit", 1000, "2026-06-10 14:02:01")
        ));
    }
}
