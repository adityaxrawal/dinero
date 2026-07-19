//! Parses Gmail's `Authentication-Results` header (RFC 8601) into a
//! structured SPF/DKIM/DMARC verdict, and uses it to cross-check Gate 1's
//! domain-string verification.
//!
//! Gate 1 previously verified a sender using only the `From` header's domain
//! string (exact match / suffix / homoglyph / edit-distance against the
//! registry) -- it never looked at whether the message's origin was
//! cryptographically authenticated at all. `Authentication-Results` is
//! already present in the exact `headers` array Gate 1 fetches for
//! From/Subject at `format=metadata` (no extra API call), and DKIM in
//! particular is a signature the sending domain's private key produced --
//! unforgeable by a spoofer who doesn't hold that key. A `From:` header that
//! string-matches a verified bank domain but whose SPF/DKIM/DMARC all failed
//! is strong evidence the visible domain was forged upstream of Gmail's own
//! authentication check (e.g. a compromised relay, a raw envelope spoof) --
//! this closes that gap without weakening anything the string checks already
//! catch (this module only ever downgrades a Verified* result to
//! SpoofReject; it never promotes an otherwise-rejected sender).

use crate::ingestion::gmail_client::MessagePartHeader;
use crate::ingestion::verified_senders::SenderVerificationResult;
use regex::Regex;
use std::sync::OnceLock;

fn verdict_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)\b(spf|dkim|dmarc)=(\w+)").unwrap())
}

fn dkim_domain_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // `header.i=@domain` (DKIM identity) or `header.d=domain` (DKIM signing
    // domain) -- either form appears depending on the signer.
    RE.get_or_init(|| Regex::new(r"(?i)header\.[id]=@?([\w.-]+)").unwrap())
}

fn dmarc_from_domain_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)header\.from=([\w.-]+)").unwrap())
}

/// Structured SPF/DKIM/DMARC verdicts extracted from one
/// `Authentication-Results` header. Each field is the raw lowercase verdict
/// token (`"pass"`, `"fail"`, `"softfail"`, `"neutral"`, `"none"`, ...) --
/// deliberately not an enum, since RFC 8601 defines more result codes than
/// this pipeline needs to branch on individually.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuthResults {
    pub spf: Option<String>,
    pub dkim: Option<String>,
    pub dmarc: Option<String>,
    /// The DKIM signing domain (`header.d=`/`header.i=`), when present.
    pub dkim_domain: Option<String>,
    /// The DMARC-aligned `From:` domain (`header.from=`), when present.
    pub dmarc_from_domain: Option<String>,
}

impl AuthResults {
    /// DMARC's own pass/fail already accounts for SPF/DKIM alignment, so an
    /// explicit `dmarc=fail` is the strongest single signal. Falling back to
    /// "both SPF and DKIM explicitly failed" covers senders with no DMARC
    /// policy published at all (`dmarc=none` or the header absent), which is
    /// still common outside large banks.
    pub fn authentication_failed(&self) -> bool {
        self.dmarc.as_deref() == Some("fail")
            || (self.spf.as_deref() == Some("fail") && self.dkim.as_deref() == Some("fail"))
    }
}

/// Parses the `Authentication-Results` header Gmail's own receiving MTA
/// attaches to inbound mail. Only trusts a header whose authserv-id (the
/// text before the first `;`) names a Google host -- an upstream/forwarding
/// relay could otherwise inject its own fabricated `Authentication-Results`
/// header before the message ever reaches Gmail, and Gate 1 must not treat a
/// spoofer-supplied verdict as if Gmail itself produced it. Returns `None`
/// when no such trusted header is present (message never authenticated by
/// Gmail's own MTA, or the header format is unrecognized) -- callers must
/// treat that as "no signal available", not as a failure.
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

/// Cross-checks a domain-string-based Gate 1 verdict against the message's
/// authentication results. Only ever downgrades: a `Verified*` result whose
/// SPF/DKIM/DMARC all failed becomes `SpoofReject`; every other input
/// (already-rejected results, or a `Verified*` result with no auth-results
/// signal available at all) passes through unchanged. Never promotes an
/// otherwise-rejected sender -- an attacker's own domain trivially passes
/// its own SPF/DKIM, so a pass verdict proves nothing about legitimacy on
/// its own, only a fail on an already-domain-verified sender is meaningful.
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
