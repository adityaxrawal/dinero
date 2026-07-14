use crate::ingestion::verified_senders::{SenderValidator, SenderVerificationResult};

#[test]
fn test_hdfc_sender_verified() {
    let validator = SenderValidator::new();

    // Normal case
    let result = validator.verify_sender("alerts@hdfcbank.net", Some("HDFC Bank"));
    assert_eq!(
        result,
        SenderVerificationResult::VerifiedTransactionCandidate("HDFC Bank".to_string())
    );

    // Without display name
    let result2 = validator.verify_sender("statements@hdfcbank.com", None);
    assert_eq!(
        result2,
        SenderVerificationResult::VerifiedStatementCandidate("HDFC Bank".to_string())
    );
}

#[test]
fn test_spoofed_hdfc_sender_rejected() {
    let validator = SenderValidator::new();

    // 1. Subdomain nesting spoofing
    let result = validator.verify_sender("alerts@hdfcbank.net.scammer.com", None);
    match result {
        SenderVerificationResult::SpoofReject(reason) => {
            assert!(reason.contains("Suspicious domain containing"));
        }
        _ => panic!("Expected SpoofReject"),
    }

    // 2. Display name mismatch
    let result2 = validator.verify_sender(
        "scammer@randomdomain.com",
        Some("HDFC Bank Customer Service"),
    );
    match result2 {
        SenderVerificationResult::SpoofReject(reason) => {
            assert!(reason.contains("Mismatched display name"));
        }
        _ => panic!("Expected SpoofReject"),
    }
}

#[test]
fn test_typo_squatted_domain_rejected() {
    let validator = SenderValidator::new();

    // Levenshtein distance <= 2 from a registered domain.
    let result = validator.verify_sender("alerts@hdfcbnk.net", None);
    match result {
        SenderVerificationResult::SpoofReject(reason) => {
            assert!(reason.contains("Typo-squatted domain"));
        }
        _ => panic!("Expected SpoofReject"),
    }
}

#[test]
fn test_homoglyph_domain_rejected() {
    let validator = SenderValidator::new();

    // Cyrillic "а" (U+0430) standing in for Latin "a" in "hdfcbank.net" —
    // renders identically in most fonts but is a different domain entirely.
    let result = validator.verify_sender("alerts@hdfcb\u{0430}nk.net", None);
    match result {
        SenderVerificationResult::SpoofReject(reason) => {
            assert!(reason.contains("Homoglyph"));
        }
        _ => panic!("Expected SpoofReject, got {:?}", result),
    }
}

#[test]
fn test_punycode_domain_rejected() {
    let validator = SenderValidator::new();

    let result = validator.verify_sender("alerts@xn--hdfcbnk-abc.net", None);
    match result {
        SenderVerificationResult::SpoofReject(reason) => {
            assert!(reason.contains("Punycode"));
        }
        _ => panic!("Expected SpoofReject, got {:?}", result),
    }
}

#[test]
fn test_unknown_sender_rejected() {
    let validator = SenderValidator::new();

    let result = validator.verify_sender("hello@example.com", None);
    match result {
        SenderVerificationResult::UnverifiedReject(reason) => {
            assert!(reason.contains("Unknown sender domain"));
        }
        _ => panic!("Expected UnverifiedReject"),
    }
}

#[test]
fn test_invalid_email_format() {
    let validator = SenderValidator::new();

    let result = validator.verify_sender("not_an_email", None);
    match result {
        SenderVerificationResult::UnverifiedReject(reason) => {
            assert!(reason.contains("Invalid email format"));
        }
        _ => panic!("Expected UnverifiedReject"),
    }
}
