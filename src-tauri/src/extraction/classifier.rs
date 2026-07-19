#[derive(Debug, PartialEq)]
pub enum ClassificationResult {
    VerifiedTransactionCandidate,
    VerifiedStatementCandidate,
    VerifiedNoise,
    UnverifiedReject,
    SpoofReject,
}

/// Gate 1: Sender Verification
pub fn verify_sender(sender: &str) -> ClassificationResult {
    // Scaffold: Trusted Indian bank domains
    let trusted_domains = vec!["hdfcbank.net", "icicibank.com", "axisbank.com", "sbi.co.in"];

    for domain in trusted_domains {
        if sender.contains(domain) {
            // Further verification based on content hints (Gate 2) will determine if it's transaction vs statement
            return ClassificationResult::VerifiedTransactionCandidate;
        }
    }

    ClassificationResult::UnverifiedReject
}

/// Gate 2 & 3: Content Classification and Mandatory Fields
pub fn classify_content(subject: &str, body: &str) -> bool {
    let has_amount = body.contains("Rs.") || body.contains("INR") || subject.contains("INR");
    let has_verb = subject.to_lowercase().contains("debited")
        || subject.to_lowercase().contains("credited")
        || subject.to_lowercase().contains("spent");

    has_amount && has_verb
}
