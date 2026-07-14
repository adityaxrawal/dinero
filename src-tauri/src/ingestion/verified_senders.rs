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

        // Subdomain nesting (e.g., alerts.hdfcbank.net.scammer.com)
        for config in &self.registry.senders {
            if domain.contains(&config.domain) {
                return SenderVerificationResult::SpoofReject(format!(
                    "Suspicious domain containing {}",
                    config.domain
                ));
            }
        }

        // Homoglyph / IDN look-alike detection: normalize confusable Unicode
        // characters (e.g. Cyrillic "а" U+0430 standing in for Latin "a")
        // and re-check for an exact match — catches domains that render
        // identically to a verified one but use a different script, which
        // plain string equality and even Levenshtein distance can miss once
        // more than 2 characters are substituted.
        if domain.chars().any(|c| !c.is_ascii()) {
            let normalized = normalize_confusables(&domain);
            if let Some(config) = self.registry.senders.iter().find(|s| s.domain == normalized) {
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

        // Levenshtein distance typo-squatting (Doc 30 TASK-GMAIL-004: "via strsim")
        for config in &self.registry.senders {
            let dist = strsim::levenshtein(&domain, &config.domain);
            // Threshold of 1 or 2 is considered a typo/spoof
            if dist <= 2 {
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

/// Maps common homoglyph/look-alike Unicode characters (mostly Cyrillic,
/// visually near-identical to Latin in most fonts) to their Latin
/// equivalent, so an IDN homograph domain normalizes to the same string a
/// verified-registry exact-match check can catch (Doc 30 TASK-GMAIL-004).
fn normalize_confusables(input: &str) -> String {
    input
        .chars()
        .map(|c| match c {
            'а' => 'a', // Cyrillic а U+0430
            'е' => 'e', // Cyrillic е U+0435
            'о' => 'o', // Cyrillic о U+043E
            'р' => 'p', // Cyrillic р U+0440
            'с' => 'c', // Cyrillic с U+0441
            'х' => 'x', // Cyrillic х U+0445
            'у' => 'y', // Cyrillic у U+0443
            'і' => 'i', // Cyrillic і U+0456
            'ѕ' => 's', // Cyrillic ѕ U+0455
            'ј' => 'j', // Cyrillic ј U+0458
            'ԁ' => 'd', // Cyrillic ԁ U+0501
            'ⅰ' => 'i', // Roman numeral one, commonly used as a look-alike
            _ => c,
        })
        .collect()
}
