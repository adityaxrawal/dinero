//! Scores how risky a sender is, combining reputation with authentication.
//!
//! Neither signal alone suffices. A domain with good history can be spoofed on a
//! given message, and a first-time legitimate bank has no history yet -- so both
//! are weighed rather than either being decisive.
use crate::db::sender_reputation::SenderReputationRow;
use crate::ingestion::auth_results::AuthResults;
use crate::ingestion::verified_senders::SenderVerificationResult;

#[derive(Debug, Clone, PartialEq)]
pub struct SenderRiskAssessment {
    pub result: SenderVerificationResult,
    pub risk_score: f64,
    pub signals: Vec<String>,
}

/// Starting risk score from the sender verification outcome.
fn base_score(result: &SenderVerificationResult) -> (f64, &'static str) {
    match result {
        SenderVerificationResult::VerifiedTransactionCandidate(name)
        | SenderVerificationResult::VerifiedStatementCandidate(name)
            if name == "Unknown Bank" =>
        {
            (0.5, "subject_rescue_no_domain_match")
        }
        SenderVerificationResult::VerifiedTransactionCandidate(_)
        | SenderVerificationResult::VerifiedStatementCandidate(_) => {
            (0.05, "registry_domain_match")
        }
        SenderVerificationResult::VerifiedNoise => (0.05, "registry_domain_match_noise"),
        SenderVerificationResult::UnverifiedReject(_) => (0.6, "unrecognized_domain"),
        SenderVerificationResult::SpoofReject(_) => (0.95, "spoof_heuristic_triggered"),
    }
}

/// Combines verification, authentication and reputation into a risk assessment.
///
/// No single signal is decisive: a domain with good history can be spoofed on a
/// given message, and a legitimate bank seen for the first time has no history at
/// all. Weighing them together is what avoids both false accepts and false
/// rejects.
pub fn assess_sender_risk(
    result: SenderVerificationResult,
    auth: Option<&AuthResults>,
    reputation: Option<&SenderReputationRow>,
) -> SenderRiskAssessment {
    let (mut score, base_signal) = base_score(&result);
    let mut signals = vec![base_signal.to_string()];

    if let Some(auth) = auth {
        if auth.authentication_failed() {
            score = (score + 0.4).min(1.0);
            signals.push("spf_dkim_dmarc_failed".to_string());
        } else if auth.dmarc.as_deref() == Some("pass") {
            score = (score - 0.1).max(0.0);
            signals.push("dmarc_pass".to_string());
        }
    }

    if let Some(rep) = reputation {
        if rep.message_count > 0 {
            let pass_rate = rep.verified_pass_count as f64 / rep.message_count as f64;
            score = (score - 0.1 * pass_rate).max(0.0);
            if pass_rate > 0.9 {
                signals.push("established_sender_history".to_string());
            }
        }
    }

    SenderRiskAssessment {
        result,
        risk_score: score.clamp(0.0, 1.0),
        signals,
    }
}
