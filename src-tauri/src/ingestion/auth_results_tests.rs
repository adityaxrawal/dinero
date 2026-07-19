use crate::ingestion::auth_results::{apply_auth_results_check, parse_authentication_results};
use crate::ingestion::gmail_client::MessagePartHeader;
use crate::ingestion::verified_senders::SenderVerificationResult;

fn header(name: &str, value: &str) -> MessagePartHeader {
    MessagePartHeader {
        name: name.to_string(),
        value: value.to_string(),
    }
}

#[test]
fn test_parses_gmail_authentication_results_all_pass() {
    let headers = vec![header(
        "Authentication-Results",
        "mx.google.com;\n       dkim=pass header.i=@hdfcbank.net header.s=selector1 header.b=abcd1234;\n       spf=pass (google.com: domain of alerts@hdfcbank.net designates 1.2.3.4 as permitted sender) smtp.mailfrom=alerts@hdfcbank.net;\n       dmarc=pass (p=REJECT sp=REJECT dis=NONE) header.from=hdfcbank.net",
    )];

    let result = parse_authentication_results(&headers).expect("should parse");
    assert_eq!(result.spf.as_deref(), Some("pass"));
    assert_eq!(result.dkim.as_deref(), Some("pass"));
    assert_eq!(result.dmarc.as_deref(), Some("pass"));
    assert_eq!(result.dkim_domain.as_deref(), Some("hdfcbank.net"));
    assert_eq!(result.dmarc_from_domain.as_deref(), Some("hdfcbank.net"));
    assert!(!result.authentication_failed());
}

#[test]
fn test_parses_dmarc_fail() {
    let headers = vec![header(
        "Authentication-Results",
        "mx.google.com; dkim=none; spf=softfail smtp.mailfrom=alerts@spoofed.example; dmarc=fail header.from=hdfcbank.net",
    )];

    let result = parse_authentication_results(&headers).expect("should parse");
    assert_eq!(result.dmarc.as_deref(), Some("fail"));
    assert!(result.authentication_failed());
}

/// A header not attributed to a Google authserv-id must be ignored --
/// otherwise an upstream/forwarding relay could inject its own fabricated
/// `Authentication-Results` header and have it trusted as if Gmail produced
/// it.
#[test]
fn test_untrusted_authserv_id_ignored() {
    let headers = vec![header(
        "Authentication-Results",
        "some-random-relay.example; dkim=pass; spf=pass; dmarc=pass",
    )];

    assert!(parse_authentication_results(&headers).is_none());
}

#[test]
fn test_no_header_present_returns_none() {
    let headers = vec![header("From", "alerts@hdfcbank.net")];
    assert!(parse_authentication_results(&headers).is_none());
}

/// The core cross-check: a domain-string-verified sender whose SPF/DKIM/DMARC
/// all failed must be downgraded to SpoofReject -- the visible From domain
/// alone was never proof the message actually came from that domain's real
/// infrastructure.
#[test]
fn test_verified_result_downgraded_on_auth_failure() {
    let headers = vec![header(
        "Authentication-Results",
        "mx.google.com; dkim=fail; spf=fail smtp.mailfrom=alerts@hdfcbank.net; dmarc=fail header.from=hdfcbank.net",
    )];

    let verified = SenderVerificationResult::VerifiedTransactionCandidate("HDFC Bank".to_string());
    let result = apply_auth_results_check(verified, &headers);

    match result {
        SenderVerificationResult::SpoofReject(reason) => {
            assert!(reason.contains("authentication failed"));
            assert!(reason.contains("HDFC Bank"));
        }
        other => panic!("Expected SpoofReject, got {:?}", other),
    }
}

/// A Verified* result with a passing (or absent) auth-results signal must
/// pass through unchanged.
#[test]
fn test_verified_result_unchanged_on_auth_pass() {
    let headers = vec![header(
        "Authentication-Results",
        "mx.google.com; dkim=pass header.i=@hdfcbank.net; spf=pass smtp.mailfrom=alerts@hdfcbank.net; dmarc=pass header.from=hdfcbank.net",
    )];

    let verified = SenderVerificationResult::VerifiedTransactionCandidate("HDFC Bank".to_string());
    let result = apply_auth_results_check(verified.clone(), &headers);
    assert_eq!(result, verified);
}

/// No Authentication-Results header at all (e.g. test fixtures, or a
/// message that genuinely lacks one) must not be treated as a failure --
/// "no signal" is not "bad signal".
#[test]
fn test_verified_result_unchanged_when_no_auth_header() {
    let headers = vec![header("From", "alerts@hdfcbank.net")];
    let verified = SenderVerificationResult::VerifiedTransactionCandidate("HDFC Bank".to_string());
    let result = apply_auth_results_check(verified.clone(), &headers);
    assert_eq!(result, verified);
}

/// Already-rejected results must never be promoted by a passing auth
/// signal -- an attacker's own domain trivially passes its own SPF/DKIM,
/// so a pass verdict proves nothing about legitimacy on its own.
#[test]
fn test_unverified_result_never_promoted_by_auth_pass() {
    let headers = vec![header(
        "Authentication-Results",
        "mx.google.com; dkim=pass header.i=@totally-unrelated.example; spf=pass; dmarc=pass header.from=totally-unrelated.example",
    )];

    let rejected = SenderVerificationResult::UnverifiedReject("Unknown sender domain".to_string());
    let result = apply_auth_results_check(rejected.clone(), &headers);
    assert_eq!(result, rejected);
}
