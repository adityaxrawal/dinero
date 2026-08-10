//! Parses the Authentication-Results header (SPF, DKIM, DMARC).
//!
//! Turns the header into a verdict on whether the sending domain is authentic.
//! This is what makes sender verification more than a name check: a spoofed
//! From address fails these, whereas the domain's own mail passes.
use crate::ingestion::gmail_client::MessagePartHeader;
use crate::ingestion::verified_senders::SenderVerificationResult;
use regex::Regex;
use std::sync::OnceLock;

/// Pattern matching an authentication verdict, compiled once.
fn verdict_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)\b(spf|dkim|dmarc)=(\w+)").unwrap())
}

/// Pattern extracting the DKIM signing domain.
fn dkim_domain_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)header\.[id]=@?([\w.-]+)").unwrap())
}

/// Pattern extracting the DMARC-evaluated From domain.
fn dmarc_from_domain_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)header\.from=([\w.-]+)").unwrap())
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuthResults {
    pub spf: Option<String>,
    pub dkim: Option<String>,
    pub dmarc: Option<String>,
    pub dkim_domain: Option<String>,
    pub dmarc_from_domain: Option<String>,
}

impl AuthResults {
    /// Whether the message failed sender authentication.
    ///
    /// DMARC failing is decisive on its own, since it evaluates alignment with the
    /// visible From domain. SPF and DKIM must both fail to count, because either one
    /// legitimately fails on forwarded mail while the other still passes.
    pub fn authentication_failed(&self) -> bool {
        self.dmarc.as_deref() == Some("fail")
            || (self.spf.as_deref() == Some("fail") && self.dkim.as_deref() == Some("fail"))
    }
}

/// Parses SPF, DKIM and DMARC verdicts from the Authentication-Results header.
///
/// This header is added by the receiving provider, not the sender, which is
/// precisely why it can be trusted: a spoofed From address fails these checks,
/// whereas a bank's own mail passes them.
pub fn parse_authentication_results(headers: &[MessagePartHeader]) -> Option<AuthResults> {
    let trusted = headers.iter().find(|h| {
        h.name.eq_ignore_ascii_case("authentication-results")
            && h.value
                .split(';')
                .next()
                .map(|authserv| authserv.to_lowercase().contains("google.com"))
                .unwrap_or(false)
    })?;

    let value = &trusted.value;
    let mut result = AuthResults::default();
    for caps in verdict_re().captures_iter(value) {
        let verdict = caps[2].to_lowercase();
        match caps[1].to_lowercase().as_str() {
            "spf" => result.spf.get_or_insert(verdict),
            "dkim" => result.dkim.get_or_insert(verdict),
            "dmarc" => result.dmarc.get_or_insert(verdict),
            _ => continue,
        };
    }
    result.dkim_domain = dkim_domain_re()
        .captures(value)
        .map(|c| c[1].to_lowercase());
    result.dmarc_from_domain = dmarc_from_domain_re()
        .captures(value)
        .map(|c| c[1].to_lowercase());

    Some(result)
}

/// Applies the parsed verdicts to a sender verification decision.
///
/// Authentication alone is not sufficient -- an authenticated domain can still be
/// a lookalike -- so this adjusts the verdict rather than deciding it.
pub fn apply_auth_results_check(
    result: SenderVerificationResult,
    headers: &[MessagePartHeader],
) -> SenderVerificationResult {
    let bank_name = match &result {
        SenderVerificationResult::VerifiedTransactionCandidate(b)
        | SenderVerificationResult::VerifiedStatementCandidate(b) => b.clone(),
        _ => return result,
    };

    let Some(auth) = parse_authentication_results(headers) else {
        return result;
    };

    if auth.authentication_failed() {
        return SenderVerificationResult::SpoofReject(format!(
            "SPF/DKIM/DMARC authentication failed for domain claiming to be {} (spf={:?}, dkim={:?}, dmarc={:?})",
            bank_name, auth.spf, auth.dkim, auth.dmarc
        ));
    }

    result
}
