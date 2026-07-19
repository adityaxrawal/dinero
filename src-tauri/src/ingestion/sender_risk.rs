//! Graded Gate 1 risk score, additive alongside `SenderVerificationResult`.
//!
//! Gate 1's decision itself stays binary (accept/reject) -- that's still the
//! right behavior for the automated pipeline, matching how Gate 3's
//! mandatory-field gate and reconciliation's viability floor are also hard
//! cutoffs, not scores, at the point where a pass/fail decision is actually
//! made. This module instead produces a continuous 0.0 (certainly
//! legitimate) .. 1.0 (certainly malicious) score for everything *around*
//! that decision -- a future review queue, telemetry/alerting on rising risk
//! for a domain over time, or ranking `pending_senders` candidates -- none of
//! which exist as a binary decision today. It never feeds back into the
//! accept/reject decision itself.

use crate::db::sender_reputation::SenderReputationRow;
use crate::ingestion::auth_results::AuthResults;
use crate::ingestion::verified_senders::SenderVerificationResult;

/// A `SenderVerificationResult` plus a continuous risk score and the named
/// signals that produced it -- the score is a summary, `signals` is why.
#[derive(Debug, Clone, PartialEq)]
pub struct SenderRiskAssessment {
    pub result: SenderVerificationResult,
    pub risk_score: f64,
    pub signals: Vec<String>,
}

/// Base risk purely from which `SenderVerificationResult` variant this is --
/// the "Unknown Bank" subject-rescue path is deliberately scored between a
/// real registry-verified pass and an outright reject: it's a real accept
/// decision (so far below reject), but one made off subject-line wording
/// alone with no domain corroboration at all (so nowhere near as low-risk as
/// an exact registry match).
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

/// Combines the base per-variant score with SPF/DKIM/DMARC and sighting
/// history, when available. Both extra inputs are optional -- callers that
/// only have the `SenderVerificationResult` (no headers parsed, no DB
/// lookup done) still get a meaningful score from the base signal alone.
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
            // Established history of mostly-verified-pass sightings lowers
            // risk, scaled small -- history corroborates, it doesn't override
            // what the current message's own signals already say.
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
