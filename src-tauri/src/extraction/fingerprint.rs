//! Computes the deduplication fingerprint for an observation.
//!
//! The fingerprint is what makes ingestion idempotent: re-scanning a mailbox
//! must not duplicate transactions. It is derived from the payment's intrinsic
//! attributes rather than from the message, so the same payment seen through two
//! different emails fingerprints identically.
use sha2::{Digest, Sha256};

/// Computes the deduplication fingerprint for an observation.
///
/// This is what makes ingestion idempotent: re-scanning a mailbox must not
/// duplicate transactions, so identity is derived from the payment's own
/// attributes rather than from the message carrying it.
///
/// The time component is bucketed to the minute deliberately. Two sources report
/// the same payment seconds apart, so hashing an exact timestamp would make
/// identical payments fingerprint differently and defeat the whole mechanism.
///
/// The account id participates so that the same payment observed through two
/// connected mailboxes is not silently collapsed into one.
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

    #[test]
    fn test_fingerprint_deterministic_for_same_inputs() {
        let a = compute_fingerprint("inst_1", "debit", 150000, "2024-01-01T10:00", "acc_1");
        let b = compute_fingerprint("inst_1", "debit", 150000, "2024-01-01T10:00", "acc_1");
        assert_eq!(a, b);
    }

    #[test]
    fn test_fingerprint_differs_across_accounts() {
        let a = compute_fingerprint("inst_1", "debit", 150000, "2024-01-01T10:00", "acc_1");
        let b = compute_fingerprint("inst_1", "debit", 150000, "2024-01-01T10:00", "acc_2");
        assert_ne!(a, b);
    }

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
