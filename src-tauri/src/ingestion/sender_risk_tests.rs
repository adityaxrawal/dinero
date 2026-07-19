use crate::db::sender_reputation::SenderReputationRow;
use crate::ingestion::auth_results::AuthResults;
use crate::ingestion::sender_risk::assess_sender_risk;
use crate::ingestion::verified_senders::SenderVerificationResult;

fn reputation(message_count: i64, verified_pass_count: i64) -> SenderReputationRow {
    let now = chrono::Utc::now().naive_utc();
    SenderReputationRow {
        domain: "hdfcbank.net".to_string(),
        first_seen_at: now,
        last_seen_at: now,
        message_count,
        verified_pass_count,
        last_verification_result: "verified_transaction_candidate".to_string(),
    }
}

#[test]
fn test_registry_match_is_low_risk() {
    let result = SenderVerificationResult::VerifiedTransactionCandidate("HDFC Bank".to_string());
    let assessment = assess_sender_risk(result, None, None);
    assert!(assessment.risk_score < 0.2, "got {}", assessment.risk_score);
}

#[test]
fn test_spoof_reject_is_high_risk() {
    let result = SenderVerificationResult::SpoofReject("Typo-squatted domain".to_string());
    let assessment = assess_sender_risk(result, None, None);
    assert!(assessment.risk_score > 0.8, "got {}", assessment.risk_score);
}

#[test]
fn test_unknown_bank_subject_rescue_is_medium_risk() {
    let result = SenderVerificationResult::VerifiedTransactionCandidate("Unknown Bank".to_string());
    let assessment = assess_sender_risk(result, None, None);
    assert!(
        assessment.risk_score > 0.3 && assessment.risk_score < 0.7,
        "got {}",
        assessment.risk_score
    );
    assert!(assessment
        .signals
        .contains(&"subject_rescue_no_domain_match".to_string()));
}

/// The core ordering claim: a domain-string match with failed
/// authentication must score strictly worse than the same match with
/// passing authentication -- this is the whole point of combining the two
/// signals into one score.
#[test]
fn test_auth_failure_increases_risk_above_auth_pass() {
    let auth_pass = AuthResults {
        spf: Some("pass".to_string()),
        dkim: Some("pass".to_string()),
        dmarc: Some("pass".to_string()),
        dkim_domain: Some("hdfcbank.net".to_string()),
        dmarc_from_domain: Some("hdfcbank.net".to_string()),
    };
    let auth_fail = AuthResults {
        spf: Some("fail".to_string()),
        dkim: Some("fail".to_string()),
        dmarc: Some("fail".to_string()),
        dkim_domain: None,
        dmarc_from_domain: None,
    };

    let pass_score = assess_sender_risk(
        SenderVerificationResult::VerifiedTransactionCandidate("HDFC Bank".to_string()),
        Some(&auth_pass),
        None,
    )
    .risk_score;
    let fail_score = assess_sender_risk(
        SenderVerificationResult::VerifiedTransactionCandidate("HDFC Bank".to_string()),
        Some(&auth_fail),
        None,
    )
    .risk_score;

    assert!(
        fail_score > pass_score,
        "fail={} should exceed pass={}",
        fail_score,
        pass_score
    );
}

#[test]
fn test_established_history_lowers_risk() {
    let no_history_score = assess_sender_risk(
        SenderVerificationResult::VerifiedTransactionCandidate("HDFC Bank".to_string()),
        None,
        None,
    )
    .risk_score;

    let established_score = assess_sender_risk(
        SenderVerificationResult::VerifiedTransactionCandidate("HDFC Bank".to_string()),
        None,
        Some(&reputation(100, 100)),
    )
    .risk_score;

    assert!(established_score < no_history_score);
}

#[test]
fn test_score_always_clamped_to_unit_interval() {
    let auth_fail = AuthResults {
        spf: Some("fail".to_string()),
        dkim: Some("fail".to_string()),
        dmarc: Some("fail".to_string()),
        dkim_domain: None,
        dmarc_from_domain: None,
    };
    let assessment = assess_sender_risk(
        SenderVerificationResult::SpoofReject("Typo-squatted domain".to_string()),
        Some(&auth_fail),
        None,
    );
    assert!(assessment.risk_score <= 1.0 && assessment.risk_score >= 0.0);
}
