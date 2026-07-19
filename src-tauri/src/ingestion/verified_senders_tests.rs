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

/// gmail false-negative remediation, Cluster A: SBI Card, IDFC FIRST Bank's
/// `idfcfirst.bank.in` subdomain, and Slice's `slice.bank.in` subdomain were
/// missing from the registry entirely -- the domains weren't spoofing
/// anything, they were simply never registered, so Gate 1's display-name
/// substring check (or the final unverified-sender fallback) correctly
/// rejected them as unrecognized. Registered, they now resolve on exact
/// match before any spoof heuristic runs at all.
#[test]
fn test_sbi_card_sender_verified() {
    let validator = SenderValidator::new();
    let result = validator.verify_sender(
        "onlinesbicard@sbicard.com",
        Some("SBI Card Transaction Alert"),
    );
    assert_eq!(
        result,
        SenderVerificationResult::VerifiedTransactionCandidate("SBI Card".to_string())
    );
}

#[test]
fn test_idfc_first_bank_rotated_statement_subdomain_verified() {
    let validator = SenderValidator::new();
    let result = validator.verify_sender("statement@idfcfirst.bank.in", Some("IDFC FIRST Bank"));
    assert_eq!(
        result,
        SenderVerificationResult::VerifiedStatementCandidate("IDFC FIRST Bank".to_string())
    );
}

#[test]
fn test_slice_rotated_statement_subdomain_verified() {
    let validator = SenderValidator::new();
    let result = validator.verify_sender("noreply@slice.bank.in", None);
    assert_eq!(
        result,
        SenderVerificationResult::VerifiedStatementCandidate(
            "Slice (North East Small Finance Bank)".to_string()
        )
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

/// Suffix-anchored subdomain fix: a genuine, never-registered subdomain of
/// an already-verified apex domain ("hdfcbank.net" is registered as
/// transaction_candidate) must now resolve as verified instead of being
/// falsely SpoofRejected by the old raw-substring nesting check -- the old
/// `domain.contains(&config.domain)` check couldn't distinguish this from a
/// real nesting attack, so every legitimate subdomain rotation required a
/// hand-added registry row.
#[test]
fn test_unregistered_subdomain_of_verified_apex_now_verified() {
    let validator = SenderValidator::new();
    let result = validator.verify_sender("noreply@notifications.hdfcbank.net", None);
    assert_eq!(
        result,
        SenderVerificationResult::VerifiedTransactionCandidate("HDFC Bank".to_string())
    );
}

/// True nesting attacks (the verified domain as a substring, NOT at the
/// suffix position) must still be rejected after the suffix-anchor fix --
/// covered already by `test_spoofed_hdfc_sender_rejected`'s
/// "hdfcbank.net.scammer.com" case; this adds a second shape (verified
/// domain in the middle of a longer random-looking host) for extra
/// confidence the suffix/substring split didn't weaken detection.
#[test]
fn test_nested_verified_domain_not_at_suffix_still_rejected() {
    let validator = SenderValidator::new();
    let result = validator.verify_sender("alerts@hdfcbank.net-secure-login.ru", None);
    match result {
        SenderVerificationResult::SpoofReject(reason) => {
            assert!(reason.contains("Suspicious domain containing"));
        }
        other => panic!("Expected SpoofReject, got {:?}", other),
    }
}

/// Length-normalized Damerau-Levenshtein catches a 3-edit typo on a long
/// domain ("unionbankofindia.bank.in", 24 chars) that the old flat
/// `dist <= 2` threshold would have missed entirely (distance here is 3),
/// even though the two domains are 87.5% similar -- an obvious typosquat at
/// this length.
#[test]
fn test_long_domain_typosquat_missed_by_old_flat_threshold_now_caught() {
    let validator = SenderValidator::new();
    let result = validator.verify_sender("alerts@xnionbanqofindiaebank.in", None);
    match result {
        SenderVerificationResult::SpoofReject(reason) => {
            assert!(reason.contains("Typo-squatted domain"));
        }
        other => panic!("Expected SpoofReject, got {:?}", other),
    }
}

/// UTS39 skeleton-based homoglyph detection covers confusables outside the
/// old 11-character hand list (Cyrillic + one Roman numeral only) -- Greek
/// small letter omicron (U+03BF) standing in for Latin "o" in "onlinesbi.com"
/// is a classic confusable the old hand-rolled map never included.
#[test]
fn test_greek_homoglyph_domain_rejected() {
    let validator = SenderValidator::new();
    let result = validator.verify_sender("alerts@\u{03bf}nlinesbi.com", None);
    match result {
        SenderVerificationResult::SpoofReject(reason) => {
            assert!(reason.contains("Homoglyph"));
        }
        other => panic!("Expected SpoofReject, got {:?}", other),
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
