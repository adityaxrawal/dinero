//! Doc 30 TASK-TXN-008: Fingerprint Generation for Observations.
//!
//! `transaction_observations.fingerprint` is the fast pre-filter/index the
//! deduplication engine (Area 7) checks before more expensive scoring logic
//! — computed from fields that identify the *real-world transaction*
//! (instrument, direction, amount, minute-bucketed time, connected account),
//! deliberately excluding anything source-specific (message ID, pipeline
//! name). Two independent observations of the same real transaction — one
//! from a Gmail alert, one from a statement row — must produce the *same*
//! fingerprint so the dedup engine can find them; including a source-unique
//! field would defeat that entirely.

use sha2::{Digest, Sha256};

/// `event_time_minute_bucket` must already be formatted to minute precision
/// (e.g. `"2024-01-01T10:00"`) by the caller — this function does not parse
/// or bucket dates itself, it only hashes.
pub fn compute_fingerprint(
    instrument_id: &str,
    direction: &str,
    amount_minor: i64,
    event_time_minute_bucket: &str,
    connected_account_id: &str,
) -> String {
    let input = format!(
        "{}|{}|{}|{}|{}",
        instrument_id, direction, amount_minor, event_time_minute_bucket, connected_account_id
    );
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Doc 30 TASK-TXN-008 acceptance test.
    #[test]
    fn test_fingerprint_deterministic_for_same_inputs() {
        let a = compute_fingerprint("inst_1", "debit", 150000, "2024-01-01T10:00", "acc_1");
        let b = compute_fingerprint("inst_1", "debit", 150000, "2024-01-01T10:00", "acc_1");
        assert_eq!(a, b);
    }

    /// Doc 30 TASK-TXN-008 acceptance test: "connected_accounts.id (to
    /// prevent cross-account false merges)" — otherwise-identical alerts
    /// from two different Gmail accounts must not fingerprint the same.
    #[test]
    fn test_fingerprint_differs_across_accounts() {
        let a = compute_fingerprint("inst_1", "debit", 150000, "2024-01-01T10:00", "acc_1");
        let b = compute_fingerprint("inst_1", "debit", 150000, "2024-01-01T10:00", "acc_2");
        assert_ne!(a, b);
    }

    /// Doc 30 TASK-TXN-008 acceptance test: "event_time (time-bucketed to
    /// the nearest minute)" — the caller is responsible for the bucketing,
    /// but the fingerprint itself must be sensitive to the bucket string it's
    /// given (two different minute buckets differ; this is what makes the
    /// bucketing meaningful at all).
    #[test]
    fn test_fingerprint_time_bucketing() {
        let same_minute_a =
            compute_fingerprint("inst_1", "debit", 150000, "2024-01-01T10:00", "acc_1");
        let same_minute_b =
            compute_fingerprint("inst_1", "debit", 150000, "2024-01-01T10:00", "acc_1");
        assert_eq!(
            same_minute_a, same_minute_b,
            "two events bucketed to the same minute must fingerprint identically"
        );

        let different_minute =
            compute_fingerprint("inst_1", "debit", 150000, "2024-01-01T10:01", "acc_1");
        assert_ne!(
            same_minute_a, different_minute,
            "a different minute bucket must change the fingerprint"
        );
    }

    #[test]
    fn test_fingerprint_differs_for_different_amount_or_direction() {
        let base = compute_fingerprint("inst_1", "debit", 150000, "2024-01-01T10:00", "acc_1");
        let diff_amount =
            compute_fingerprint("inst_1", "debit", 200000, "2024-01-01T10:00", "acc_1");
        let diff_direction =
            compute_fingerprint("inst_1", "credit", 150000, "2024-01-01T10:00", "acc_1");
        let diff_instrument =
            compute_fingerprint("inst_2", "debit", 150000, "2024-01-01T10:00", "acc_1");
        assert_ne!(base, diff_amount);
        assert_ne!(base, diff_direction);
        assert_ne!(base, diff_instrument);
    }
}
