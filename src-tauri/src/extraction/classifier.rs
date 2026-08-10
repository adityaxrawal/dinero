//! Decides whether a message is financial before extraction runs.
//!
//! The first gate in ingestion, and the cheapest place to reject the bulk of a
//! mailbox. Sender verification and content classification are separate checks:
//! a genuine bank domain sending a marketing email is still not a transaction,
//! and a transaction-shaped message from an unverified sender is a phishing risk
//! rather than data to ingest.
#[derive(Debug, PartialEq)]
pub enum ClassificationResult {
    VerifiedTransactionCandidate,
    VerifiedStatementCandidate,
    VerifiedNoise,
    UnverifiedReject,
    SpoofReject,
}

/// Judges whether a sender is a genuine financial institution.
///
/// The first and cheapest gate in ingestion. Runs before content is examined at
/// all, because a transaction-shaped message from an unverified sender is a
/// phishing attempt rather than data to ingest.
pub fn verify_sender(sender: &str) -> ClassificationResult {
    let trusted_domains = vec!["hdfcbank.net", "icicibank.com", "axisbank.com", "sbi.co.in"];

    for domain in trusted_domains {
        if sender.contains(domain) {
            return ClassificationResult::VerifiedTransactionCandidate;
        }
    }

    ClassificationResult::UnverifiedReject
}

/// Whether a message's content describes a transaction.
///
/// Applied after the sender is trusted, since a verified bank also sends
/// marketing, statements and service notices. Verifying the sender establishes
/// authenticity, not relevance.
pub fn classify_content(subject: &str, body: &str) -> bool {
    let has_amount = body.contains("Rs.") || body.contains("INR") || subject.contains("INR");
    let has_verb = subject.to_lowercase().contains("debited")
        || subject.to_lowercase().contains("credited")
        || subject.to_lowercase().contains("spent");

    has_amount && has_verb
}
