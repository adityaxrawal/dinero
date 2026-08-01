//! Doc 30 TASK-DEDUP-002: Exact-Match Decision Path.
//!
//! On a fingerprint pre-filter hit (TASK-DEDUP-001), verify a strict
//! condition before auto-merging: identical `instrument_id`, `direction`,
//! `amount_minor` (to the paisa), `currency` (audit_03 #3 — the fingerprint
//! does not hash it, so same-amount different-currency alerts collide), and
//! `event_time` within +/-2 minutes
//! (accounting for email delivery lag vs. actual authorization time). All
//! conditions hold -> the caller writes
//! `match_decisions(decision = 'auto_matched_exact', score = 1.0)`. A rare
//! fingerprint collision (conditions fail despite the hash match) must fall
//! through to full scoring rather than assuming a match.

use crate::reconciliation::engine::{CanonicalCandidate, IncomingObservation};
use chrono::NaiveDateTime;

/// Doc 30 TASK-DEDUP-002: +/-2 minutes, accounting for email delivery lag
/// vs. the transaction's actual authorization time.
const EXACT_MATCH_TIME_TOLERANCE_SECONDS: i64 = 120;

/// Verifies the strict exact-match condition between an incoming observation
/// and the canonical candidate a fingerprint pre-filter hit pointed at.
///
/// audit_03 #3: `currency` is part of the condition. `compute_fingerprint`
/// deliberately does not hash it, so a ₹500.00 and a $500.00 debit on the same
/// instrument in the same minute bucket produce the *same* fingerprint — this
/// check is what stops that hash collision from auto-merging two unrelated
/// transactions. Treating it as a collision (rather than widening the
/// fingerprint) is the designed behaviour: a failed strict check falls through
/// to full scoring, and no already-stored fingerprint is invalidated.
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

    /// audit_03 #3: `compute_fingerprint` deliberately omits currency, so a
    /// ₹500.00 and a $500.00 debit on the same instrument in the same minute
    /// bucket hash identically. This strict check is the only thing standing
    /// between that hash collision and an auto-merge of two unrelated
    /// transactions, so it must compare currency.
    #[test]
    fn test_exact_match_rejects_currency_mismatch() {
        let o = obs("inst_1", "debit", 50000, "2026-06-10 14:00:00");
        let mut c = candidate("inst_1", "debit", 50000, "2026-06-10 14:00:30");
        // Identical on every other axis the fingerprint hashes.
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

    /// Doc 30 TASK-DEDUP-002 acceptance test: all conditions hold.
    #[test]
    fn test_exact_match_all_conditions_hold() {
        let o = obs("inst_1", "debit", 1000, "2026-06-10 14:00:00");
        // 90 seconds later -- within the +/-2 minute tolerance.
        let c = candidate("inst_1", "debit", 1000, "2026-06-10 14:01:30");
        assert!(verify_exact_match(&o, &c));
    }

    /// Doc 30 TASK-DEDUP-002 acceptance test: a fingerprint hash collision
    /// where the underlying fields don't actually agree must fail the strict
    /// check and fall through to full scoring rather than assuming a match.
    #[test]
    fn test_exact_match_fingerprint_collision_falls_through() {
        let o = obs("inst_1", "debit", 1000, "2026-06-10 14:00:00");

        // Different amount.
        assert!(!verify_exact_match(
            &o,
            &candidate("inst_1", "debit", 2000, "2026-06-10 14:00:30")
        ));
        // Different instrument.
        assert!(!verify_exact_match(
            &o,
            &candidate("inst_2", "debit", 1000, "2026-06-10 14:00:30")
        ));
        // Different direction.
        assert!(!verify_exact_match(
            &o,
            &candidate("inst_1", "credit", 1000, "2026-06-10 14:00:30")
        ));
        // Outside the +/-2 minute window (5 minutes later).
        assert!(!verify_exact_match(
            &o,
            &candidate("inst_1", "debit", 1000, "2026-06-10 14:05:00")
        ));
    }

    #[test]
    fn test_exact_match_time_boundary() {
        let o = obs("inst_1", "debit", 1000, "2026-06-10 14:00:00");
        // Exactly 120 seconds later -- boundary, must hold.
        assert!(verify_exact_match(
            &o,
            &candidate("inst_1", "debit", 1000, "2026-06-10 14:02:00")
        ));
        // 121 seconds later -- just outside, must fail.
        assert!(!verify_exact_match(
            &o,
            &candidate("inst_1", "debit", 1000, "2026-06-10 14:02:01")
        ));
    }
}
