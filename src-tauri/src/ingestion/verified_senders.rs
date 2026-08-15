//! Establishes whether a message genuinely came from a bank.
//!
//! The primary defence against ingesting phishing mail as real transactions. A
//! registry of known financial domains is combined with authentication results,
//! since a display name and a convincing template cost an attacker nothing --
//! only the authenticated sending domain is meaningful evidence.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SenderVerificationResult {
    VerifiedTransactionCandidate(String),
    VerifiedStatementCandidate(String),
    VerifiedNoise,
    UnverifiedReject(String),
    SpoofReject(String),
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
    /// Loads the registry bundled with the application.
    fn default() -> Self {
        Self::new()
    }
}

impl SenderValidator {
    /// Builds a validator over the verified-sender registry.
    pub fn new() -> Self {
        let registry_str = include_str!("verified_senders_registry.json");
        let registry: VerifiedSenderRegistry = serde_json::from_str(registry_str)
            .expect("Failed to parse bundled verified_senders_registry.json");
        Self { registry }
    }

    /// Every domain in the registry, used to scope Gmail queries.
    ///
    /// Scoping by sender pushes the selection into Gmail's index, which is far
    /// cheaper than fetching a date range and filtering locally.
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

    /// Short institution name for a sender address, if recognised.
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

    /// All known display names, used to spot lookalike senders.
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

    /// Decides whether a sender is a genuine financial institution.
    ///
    /// The registry establishes which domains are legitimate; a matching display name
    /// on a different domain is the signature of a phishing attempt rather than
    /// evidence in the sender's favour.
    pub fn verify_sender(
        &self,
        email_address: &str,
        display_name: Option<&str>,
    ) -> SenderVerificationResult {
        let email_address = email_address.trim();
        let (_local_part, domain_part) = match email_address.rsplit_once('@') {
            Some((local, domain)) => (local, domain),
            None => return SenderVerificationResult::UnverifiedReject("Invalid email format".into()),
        };

        use unicode_normalization::UnicodeNormalization;
        let mut domain = domain_part.trim().trim_end_matches('.').to_lowercase();
        domain = domain.nfkc().collect::<String>();
        let display_name_lower = display_name.map(|d| d.trim().to_lowercase().nfkc().collect::<String>());

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

        for config in &self.registry.senders {
            if domain.contains(&config.domain) {
                return SenderVerificationResult::SpoofReject(format!(
                    "Suspicious domain containing {}",
                    config.domain
                ));
            }
        }

        if !domain.is_ascii() {
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
        if domain.split('.').any(|label| label.starts_with("xn--")) {
            return SenderVerificationResult::SpoofReject(
                "Punycode-encoded domain (possible IDN homograph attack)".into(),
            );
        }

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

        SenderVerificationResult::UnverifiedReject("Unknown sender domain".into())
    }
}
