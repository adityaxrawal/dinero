use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SenderVerificationResult {
    VerifiedTransactionCandidate(String), // bank_name
    VerifiedStatementCandidate(String),   // bank_name
    VerifiedNoise,
    UnverifiedReject(String), // String contains reason
    SpoofReject(String),      // String contains reason
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VerifiedSenderConfig {
    pub domain: String,
    pub bank_name: String,
    pub display_names: Vec<String>,
    pub classification: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VerifiedSenderRegistry {
    pub senders: Vec<VerifiedSenderConfig>,
}

pub struct SenderValidator {
    registry: VerifiedSenderRegistry,
}

impl Default for SenderValidator {
    fn default() -> Self {
        Self::new()
    }
}

impl SenderValidator {
    pub fn new() -> Self {
        let registry_str = include_str!("verified_senders_registry.json");
        let registry: VerifiedSenderRegistry = serde_json::from_str(registry_str)
            .expect("Failed to parse bundled verified_senders_registry.json");
        Self { registry }
    }

    /// Every apex domain in the bundled registry, deduped and sorted.
    ///
    /// Used to push Gate 1 server-side: a historical scan asks Gmail for mail
    /// `from:` these domains rather than downloading metadata for the whole
    /// mailbox and rejecting ~80% of it locally at full quota cost. Gmail's
    /// `from:` matches subdomains and substrings, so genuine subdomains
    /// (`alerts.hdfcbank.net`) and nesting-attack lookalikes
    /// (`hdfcbank.net.evil.com`) are both still returned and still run through
    /// the full local `verify_sender` gate — the filter narrows what we pay to
    /// fetch, it never becomes the security decision.
    pub fn registry_domains(&self) -> Vec<String> {
        let mut domains: Vec<String> = self
            .registry
            .senders
            .iter()
            .map(|s| s.domain.to_lowercase())
            .collect();
        domains.sort();
        domains.dedup();
        domains
    }

    /// Issue #9: the shortest name this registry knows the sender's bank by —
    /// "SBI" rather than "State Bank of India" — for labelling a statement in
    /// the Action Needed queue.
    ///
    /// Deliberately a separate lookup from `verify_sender` rather than a
    /// refactor of it. That function's domain matching is load-bearing for
    /// spoof rejection, and this one answers a purely cosmetic question; a
    /// naming helper must never become a place where a security decision can
    /// be accidentally changed. It is correspondingly read-only and grants
    /// nothing: an attacker who gets a lookalike domain to resolve here wins
    /// only a nicer label on a row that is still blocked.
    pub fn short_name_for_sender(&self, email_address: &str) -> Option<String> {
        let domain = email_address.rsplit_once('@')?.1.trim().to_lowercase();
        let config = self
            .registry
            .senders
            .iter()
            .filter(|s| domain == s.domain || domain.ends_with(&format!(".{}", s.domain)))
            .max_by_key(|s| s.domain.len())?;
        Some(
            config
                .display_names
                .iter()
                .min_by_key(|n| n.len())
                .unwrap_or(&config.bank_name)
                .clone(),
        )
    }

    /// Every name any registered bank is known by, for matching an issuer in
    /// free text when the sender domain is unrecognised (a forwarded
    /// statement, or a manually uploaded file).
    pub fn all_display_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .registry
            .senders
            .iter()
            .flat_map(|s| {
                s.display_names
                    .iter()
                    .cloned()
                    .chain(std::iter::once(s.bank_name.clone()))
            })
            .collect();
        names.sort();
        names.dedup();
        names
    }

    /// Verifies the sender email address and display name against the registry
    pub fn verify_sender(
        &self,
        email_address: &str,
        display_name: Option<&str>,
    ) -> SenderVerificationResult {
        let email_parts: Vec<&str> = email_address.split('@').collect();
        if email_parts.len() != 2 {
            return SenderVerificationResult::UnverifiedReject("Invalid email format".into());
        }

        let domain = email_parts[1].to_lowercase();
        let display_name_lower = display_name.map(|d| d.to_lowercase());

        // 1. Check for exact match in registry
        if let Some(config) = self.registry.senders.iter().find(|s| s.domain == domain) {
            return match config.classification.as_str() {
                "transaction_candidate" => {
                    SenderVerificationResult::VerifiedTransactionCandidate(config.bank_name.clone())
                }
                "statement_candidate" => {
                    SenderVerificationResult::VerifiedStatementCandidate(config.bank_name.clone())
                }
                _ => SenderVerificationResult::VerifiedNoise,
            };
        }

        // 2. Spoof detection: Domain is not verified, but looks similar or uses subdomains of verified domains

        // Genuine subdomain of an already-verified apex (e.g.
        // "alerts.hdfcbank.net" when "hdfcbank.net" is registered) is
        // first-party infrastructure under that bank's own DNS -- SPF/DKIM
        // for such subdomains are configured by the bank itself, so the
        // suffix relationship is safe to trust and avoids requiring every
        // subdomain rotation to be hand-added to the registry. This is
        // distinct from a verified domain appearing as a substring anywhere
        // else (e.g. "hdfcbank.net.scammer.com", handled below) -- the
        // verified domain isn't at the suffix position there, so it can't be
        // legitimate first-party infrastructure. Longest-matching registry
        // domain wins when more than one is a suffix, so the most specific
        // verified entry governs classification.
        let mut best_suffix_match: Option<&VerifiedSenderConfig> = None;
        for config in &self.registry.senders {
            if domain.ends_with(&format!(".{}", config.domain))
                && best_suffix_match
                    .map(|c| config.domain.len() > c.domain.len())
                    .unwrap_or(true)
            {
                best_suffix_match = Some(config);
            }
        }
        if let Some(config) = best_suffix_match {
            return match config.classification.as_str() {
                "transaction_candidate" => {
                    SenderVerificationResult::VerifiedTransactionCandidate(config.bank_name.clone())
                }
                "statement_candidate" => {
                    SenderVerificationResult::VerifiedStatementCandidate(config.bank_name.clone())
                }
                _ => SenderVerificationResult::VerifiedNoise,
            };
        }

        // True nesting attack: a verified domain appears as a substring but
        // NOT at the suffix position -- e.g. "hdfcbank.net.scammer.com"
        // contains "hdfcbank.net" in the middle, not at the end.
        for config in &self.registry.senders {
            if domain.contains(&config.domain) {
                return SenderVerificationResult::SpoofReject(format!(
                    "Suspicious domain containing {}",
                    config.domain
                ));
            }
        }

        // Homoglyph / confusable-script detection via Unicode Technical
        // Standard #39's skeleton algorithm (`unicode-security` crate) --
        // the actual standard confusables table, rather than an 11-character
        // hand-picked list (previously Cyrillic + one Roman numeral only).
        // Two strings that render identically or near-identically map to the
        // same skeleton regardless of which script(s) they borrow confusable
        // glyphs from (Greek, full-width forms, etc. are all covered, not
        // just Cyrillic) -- this is what plain string equality and even
        // Levenshtein distance can miss once more than 2-3 characters are
        // substituted from a different script.
        if domain.chars().any(|c| !c.is_ascii()) {
            let candidate_skeleton: String = unicode_security::skeleton(&domain).collect();
            if let Some(config) = self.registry.senders.iter().find(|s| {
                let registry_skeleton: String = unicode_security::skeleton(&s.domain).collect();
                registry_skeleton == candidate_skeleton
            }) {
                return SenderVerificationResult::SpoofReject(format!(
                    "Homoglyph/look-alike domain mimicking {}",
                    config.domain
                ));
            }
        }
        // Punycode-encoded (IDN ACE) labels are inherently suspicious here —
        // none of our legitimate registry domains are internationalized.
        if domain.split('.').any(|label| label.starts_with("xn--")) {
            return SenderVerificationResult::SpoofReject(
                "Punycode-encoded domain (possible IDN homograph attack)".into(),
            );
        }

        // Typo-squatting via length-normalized Damerau-Levenshtein similarity
        // (Doc 30 TASK-GMAIL-004: "via strsim"). A flat edit-distance
        // threshold (the previous `dist <= 2`) is miscalibrated across
        // domain lengths: 2 edits out of a 6-char domain is a near-total
        // rewrite (should NOT flag), while 2 edits out of a 20-char domain
        // is a near-exact typo (should flag). `normalized_damerau_levenshtein`
        // returns a 0..1 similarity already scaled by the longer string's
        // length, and Damerau (vs plain Levenshtein) also catches adjacent
        // transpositions ("hdfcbnak.com") as a single edit, the single most
        // common real-world typo pattern that plain Levenshtein counts as two.
        const TYPOSQUAT_SIMILARITY_THRESHOLD: f64 = 0.85;
        for config in &self.registry.senders {
            let similarity = strsim::normalized_damerau_levenshtein(&domain, &config.domain);
            if similarity >= TYPOSQUAT_SIMILARITY_THRESHOLD && domain != config.domain {
                return SenderVerificationResult::SpoofReject(format!(
                    "Typo-squatted domain similar to {}",
                    config.domain
                ));
            }
        }

        // Display name spoofing (e.g., display name says "HDFC Bank" but domain is random)
        if let Some(dn) = &display_name_lower {
            for config in &self.registry.senders {
                for valid_name in &config.display_names {
                    if dn.contains(&valid_name.to_lowercase()) {
                        return SenderVerificationResult::SpoofReject(format!(
                            "Mismatched display name for {}",
                            config.domain
                        ));
                    }
                }
            }
        }

        // 3. Fallback: Unknown sender
        SenderVerificationResult::UnverifiedReject("Unknown sender domain".into())
    }
}
