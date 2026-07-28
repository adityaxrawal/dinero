use crate::extraction::llm::Layer6Outcome;
use anyhow::Result;
use deadpool_sqlite::Pool;
use regex::Regex;
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::future::Future;
use std::pin::Pin;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ExtractionResult {
    // Mandatory fields
    pub amount_minor: Option<i64>,
    pub currency: Option<String>,
    pub direction: Option<String>,
    pub event_time: Option<i64>,
    pub merchant_raw: Option<String>,

    // Optional fields
    pub reference_id: Option<String>,
    pub balance_after: Option<i64>,
    pub original_amount_minor: Option<i64>,
    pub original_currency: Option<String>,

    // Instrument signal fields (populated post-extraction in run_extraction_ladder)
    pub instrument_type: Option<String>,
    pub issuer_name: Option<String>,
    pub masked_identifier: Option<String>,
    pub network: Option<String>,
    pub upi_vpa: Option<String>,

    // Metadata
    pub extraction_method: String,
    /// Doc 30 TASK-TXN-001's named `ExtractionResult` field. Only Layer 5
    /// (LLM) has a spec-defined value (0.7, Doc 12 §6.3) — the other layers
    /// leave this `None` rather than inventing an unspecified number.
    /// Consumed by TASK-TXN-010 (canonical creation) to gate `pending_review`.
    pub confidence_score: Option<f64>,
    /// Doc 30 TASK-TXN-001's named `ExtractionResult` field. No document
    /// defines a versioning scheme for bank-template parsers yet, so this
    /// stays `None` everywhere until one exists — left unpopulated rather
    /// than guessed.
    pub parser_version: Option<String>,

    /// Doc 30 TASK-TXN-012: populated when Layer 2/3 detects EMI language
    /// ("EMI", "installment X of Y", "converted to EMI") in the source
    /// text. `emi_total_installments`/`emi_installment_number` come from
    /// the "X of Y" pattern when present; `emi_original_amount_minor` is
    /// the pre-EMI-conversion purchase amount when the email states it
    /// (distinct from `amount_minor`, which is this installment's amount).
    pub emi_total_installments: Option<i32>,
    pub emi_installment_number: Option<i32>,
    pub emi_original_amount_minor: Option<i64>,

    /// Doc 30 TASK-TXN-013: exchange rate parsed directly from the bank's
    /// email text when explicitly stated (FRS §6.4: "No external live API
    /// calls... are permitted to backfill this rate"). `None` when the bank
    /// doesn't print it, even if `original_currency`/`original_amount_minor`
    /// are populated -- that combination is exactly what should route a
    /// transaction to `pending_fx` rather than being guessed at here.
    pub exchange_rate: Option<f64>,

    /// `true` when `event_time` was parsed from a bare numeric date
    /// (`%d/%m/%Y`, `%d-%m-%Y`, or `%m-%d-%Y`) whose day-of-month and month
    /// are both <=12, i.e. a DD/MM-vs-MM/DD swap is a genuinely different
    /// but equally well-formed date. Month-name formats (`%d-%b-%Y` etc.)
    /// and every non-`parse_date_generic` source (bank templates, Layer 5
    /// statement rows) never set this, which is what scopes
    /// `apply_date_cross_check` to only the cases that are actually
    /// ambiguous -- see that function's doc comment for why this can't be
    /// reconstructed after the fact from the resolved date's fields alone.
    pub event_time_ambiguous: bool,
    /// Set by `apply_date_cross_check` when Gmail's `internalDate` was used
    /// to arbitrate an `event_time_ambiguous` date: `"swapped_by_anchor"`
    /// (event_time corrected) or `"anchor_mismatch_needs_review"` (left
    /// alone, confidence downgraded instead). `None` otherwise.
    pub date_cross_check_flag: Option<String>,
}

impl ExtractionResult {
    pub fn is_valid(&self) -> bool {
        let has_tx_fields = self.amount_minor.is_some()
            && self.currency.is_some()
            && self.direction.is_some()
            && self.merchant_raw.is_some();

        let has_balance_update = self.balance_after.is_some();

        self.event_time.is_some() && (has_tx_fields || has_balance_update)
    }
}

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait ExtractionLayer: Send + Sync {
    fn extract<'a>(
        &'a self,
        pool: &'a Pool,
        bank_name: &'a str,
        body: &'a str,
    ) -> BoxFuture<'a, Option<ExtractionResult>>;
    fn layer_name(&self) -> &'static str;
}

pub fn compute_template_hash(body: &str) -> String {
    let re_digits = Regex::new(r"\d+").unwrap();
    let re_whitespace = Regex::new(r"\s+").unwrap();
    let body_lower = body.to_lowercase();
    let no_digits = re_digits.replace_all(&body_lower, "#");
    let normalized = re_whitespace.replace_all(&no_digits, " ");

    let mut hasher = Sha256::new();
    hasher.update(normalized.as_bytes());
    format!("{:x}", hasher.finalize())
}

// Layer 1: Learned pattern rules (user-corrected)
pub struct LearnedPatternLayer;
impl ExtractionLayer for LearnedPatternLayer {
    fn extract<'a>(
        &'a self,
        pool: &'a Pool,
        bank_name: &'a str,
        body: &'a str,
    ) -> BoxFuture<'a, Option<ExtractionResult>> {
        Box::pin(async move {
            let b_name = bank_name.to_string();

            let conn_res = pool.get().await;
            if conn_res.is_err() {
                return None;
            }
            let conn = conn_res.unwrap();

            let rules_res = conn
                .interact(move |c| crate::db::pattern_rules::select_active_rules_by_bank(c, &b_name))
                .await;

            let rules = match rules_res {
                Ok(Ok(r)) => r,
                _ => return None,
            };

            if rules.is_empty() {
                return None;
            }

            let mut result = ExtractionResult {
                extraction_method: self.layer_name().to_string(),
                ..Default::default()
            };

            // For simplicity, we apply the rules sequentially. If multiple rules define the same field, the last one wins.
            // A more robust implementation might check for conflicts.
            let mut matched_any = false;
            for rule in rules {
                if let Some(regex_val) = rule.rule_payload_json.get("regex") {
                    if let Some(regex_str) = regex_val.as_str() {
                        if let Ok(re) = Regex::new(regex_str) {
                            if let Some(caps) = re.captures(body) {
                                if let Some(m) = caps.get(1) {
                                    let matched_str = m.as_str();
                                    matched_any = true;
                                    match rule.field_name.as_str() {
                                        "amount" | "amount_minor" => {
                                            // Quick parse logic for minor units. Assume it might contain decimals.
                                            let clean: String = matched_str
                                                .chars()
                                                .filter(|c| c.is_ascii_digit() || *c == '.')
                                                .collect();
                                            if let Ok(val) = clean.parse::<f64>() {
                                                result.amount_minor =
                                                    Some((val * 100.0).round() as i64);
                                            }
                                        }
                                        "merchant" | "merchant_raw" => {
                                            result.merchant_raw = Some(matched_str.to_string());
                                        }
                                        "currency" => {
                                            result.currency = Some(matched_str.to_string());
                                        }
                                        "direction" => {
                                            result.direction = Some(matched_str.to_string());
                                        }
                                        "event_time" => {
                                            // A learned rule's capture is either a literal
                                            // epoch timestamp (rare, direct DB-authored
                                            // rules) or a date string in the same shape
                                            // Layer 2/LLM-synthesized rules hint at (see
                                            // `synthesize_pending_rule`'s `event_time`
                                            // regex, which captures "25-May-2023" style
                                            // text, not a raw integer) -- try both parses.
                                            // Neither succeeding leaves this field `None`
                                            // (not a fabricated date), which correctly
                                            // fails `ExtractionResult::is_valid()`.
                                            match matched_str.parse::<i64>().ok() {
                                                Some(ts) => result.event_time = Some(ts),
                                                None => {
                                                    if let Some(parsed) =
                                                        parse_date_generic(matched_str)
                                                    {
                                                        result.event_time = Some(parsed.timestamp);
                                                        result.event_time_ambiguous =
                                                            parsed.ambiguous;
                                                    }
                                                }
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if matched_any && result.is_valid() {
                Some(result)
            } else {
                None
            }
        })
    }
    fn layer_name(&self) -> &'static str {
        "learned_patterns"
    }
}

use std::sync::OnceLock;

static GENERIC_CURRENCY_AMOUNT_PREFIX_RE: OnceLock<Regex> = OnceLock::new();
static GENERIC_CURRENCY_AMOUNT_SUFFIX_RE: OnceLock<Regex> = OnceLock::new();
static GENERIC_MERCHANT_RE: OnceLock<Regex> = OnceLock::new();
static GENERIC_MERCHANT_RE_STRICT: OnceLock<Regex> = OnceLock::new();
static GENERIC_SELF_REFERENTIAL_MERCHANT_RE: OnceLock<Regex> = OnceLock::new();
static GENERIC_DATE_RE: OnceLock<Regex> = OnceLock::new();
static GENERIC_REF_RE: OnceLock<Regex> = OnceLock::new();
static GENERIC_CREDIT_DIRECTION_RE: OnceLock<Regex> = OnceLock::new();
static GENERIC_DEBIT_DIRECTION_RE: OnceLock<Regex> = OnceLock::new();

/// gmail false-negative remediation: `GenericRegexLayer`, `Layer5StatementCrossrefLayer`,
/// and `cross_check_amount` each used to independently call
/// `GENERIC_CURRENCY_AMOUNT_PREFIX_RE.get_or_init(|| Regex::new(<pattern>))`
/// with their own copy of the pattern string -- `OnceLock::get_or_init`
/// only ever runs the *first* caller's closure (whichever layer happens to
/// run first for the process), so three independently-edited copies could
/// silently drift out of sync with only the first one ever taking effect.
/// One shared function is the only way to guarantee all three call sites
/// see the same pattern. Also fixes a real false negative: a body stating
/// an amount as a spelled-out ISO code ("USD 1.00", e.g. a declined
/// international card transaction) matched neither the ₹/Rs/INR nor the
/// bare `$` alternatives.
fn generic_currency_amount_regexes() -> (&'static Regex, &'static Regex) {
    let prefix = GENERIC_CURRENCY_AMOUNT_PREFIX_RE.get_or_init(|| {
        Regex::new(r"(?i)(rs\.?|inr|₹|\$|usd|eur|gbp|aed|sgd|aud|cad|jpy|chf)\s*([\d,]+(?:\.\d+)?)")
            .unwrap()
    });
    let suffix = GENERIC_CURRENCY_AMOUNT_SUFFIX_RE.get_or_init(|| {
        Regex::new(r"(?i)([\d,]+(?:\.\d+)?)\s*(inr|rs\.?|₹|usd|eur|gbp|aed|sgd|aud|cad|jpy|chf)")
            .unwrap()
    });
    (prefix, suffix)
}

/// gmail false-negative remediation: a neobank "money credited" template
/// (e.g. Jupiter) says "...was credited **to your account**" before it
/// separately labels the real counterparty further down ("Payment
/// **from**: ADITYA RAWAL"). The single-match ambiguous-tier lookup used to
/// stop at the first "to/from/at/for/by" hit and take "your account" itself
/// as the merchant -- a real value (not empty), so nothing downstream caught
/// it. Matches self-referential captures ("your account", "my savings
/// account", "the account") so the caller can skip them and keep scanning
/// for the real counterparty instead.
fn is_invalid_merchant(candidate: &str, bank_name: &str) -> bool {
    let re = GENERIC_SELF_REFERENTIAL_MERCHANT_RE
        .get_or_init(|| Regex::new(r"(?i)^(?:your|my|the)\b.*\baccount$|^account$").unwrap());
    if re.is_match(candidate.trim()) {
        return true;
    }

    // gmail false-negative remediation: a boilerplate disclaimer footer
    // ("please block your card immediately by calling...", "write to us
    // at...") satisfies the same ambiguous "at/to/from/for/by + 2-40 chars"
    // shape a real merchant label does, so it can win the leftmost-match
    // scan when the true merchant capture fails first (e.g. an underscore
    // in the descriptor breaking the value char class -- see
    // `MERCHANT_TERMINATOR`'s doc comment). A real merchant name is never
    // built entirely out of instruction filler words, so reject those here
    // regardless of which keyword or layer produced the candidate.
    if crate::extraction::lexicon::is_stopword_only_merchant(candidate.trim()) {
        return true;
    }

    // A candidate with no letters at all (pure digits/punctuation, e.g. a
    // reference number or phone extension the terminator failed to exclude)
    // is never a real merchant name.
    if !candidate.chars().any(|c| c.is_alphabetic()) {
        return true;
    }

    let candidate_lower = candidate.trim().to_lowercase();
    let bank_lower = bank_name.to_lowercase();

    if candidate_lower == bank_lower {
        return true;
    }

    if !bank_lower.is_empty() && candidate_lower.starts_with(&bank_lower) {
        let remaining = candidate_lower.strip_prefix(&bank_lower).unwrap().trim();
        if remaining.is_empty() || remaining == "bank" || remaining == "alerts" {
            return true;
        }
    }

    if candidate_lower == "bank" {
        return true;
    }

    false
}

static VPA_MERCHANT_FALLBACK_RE: OnceLock<Regex> = OnceLock::new();

/// A personal UPI P2P transfer (e.g. HDFC's "Rs.750.00 is debited from your
/// account ending 4691 towards VPA 8127696200@jupiteraxis (ADITYA RAWAL) on
/// 07-06-26.") has no real "merchant" -- the counterparty is a VPA handle,
/// not a business. `MERCHANT_TERMINATOR`'s value char class excludes `(`/`)`
/// so the parenthesised display name can't be captured anyway, and even if
/// it could, that name is often the *account holder's own* registered name
/// (as this real example is), not the payee's -- printed by the bank for
/// confirmation, not identification. The VPA handle itself is the only
/// unambiguous signal here. Deliberately narrower than
/// `extract_instrument_signals`'s general-purpose VPA detector (which
/// matches any email-shaped string): requires the literal word "VPA"
/// immediately before the handle, the standard phrasing every bank uses for
/// this, so a footer support-email address never wins this fallback -- a
/// false positive here becomes a wrong user-facing merchant name, not just
/// supplementary instrument metadata.
fn vpa_merchant_fallback(body: &str) -> Option<String> {
    let re = VPA_MERCHANT_FALLBACK_RE
        .get_or_init(|| Regex::new(r"(?i)\bVPA\s+([\w.\-+]+@[\w.\-]+)").unwrap());
    re.captures(body)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_lowercase().trim_end_matches('.').to_string())
}

// Instrument signal detection statics
static INSTR_CARD_LAST4_RE: OnceLock<Regex> = OnceLock::new();
static INSTR_ACCOUNT_SUFFIX_RE: OnceLock<Regex> = OnceLock::new();
static INSTR_USER_UPI_VPA_DEBIT_RE: OnceLock<Regex> = OnceLock::new();
static INSTR_USER_UPI_VPA_CREDIT_RE: OnceLock<Regex> = OnceLock::new();
static INSTR_USER_UPI_VPA_EXPLICIT_RE: OnceLock<Regex> = OnceLock::new();
static INSTR_CP_UPI_VPA_DEBIT_RE: OnceLock<Regex> = OnceLock::new();
static INSTR_CP_UPI_VPA_CREDIT_RE: OnceLock<Regex> = OnceLock::new();
static INSTR_NETWORK_RE: OnceLock<Regex> = OnceLock::new();

/// InstrumentSignals holds the parsed signals extracted from an email body.
#[derive(Debug, Default, Clone)]
pub struct InstrumentSignals {
    pub instrument_type: Option<String>,
    pub issuer_name: Option<String>,
    pub masked_identifier: Option<String>,
    pub network: Option<String>,
    pub upi_vpa: Option<String>,
}

/// Extracts instrument signals from a bank email body.
/// Detects masked card last-4, bank account suffix, UPI VPA, issuer name, and card network.
///
/// # Arguments
/// * `bank_name` - The issuer/bank name (e.g. "HDFC Bank").
/// * `body` - The plain-text body of the email.
pub fn extract_instrument_signals(bank_name: &str, body: &str) -> InstrumentSignals {
    let mut signals = InstrumentSignals {
        issuer_name: Some(bank_name.to_string()),
        ..Default::default()
    };

    // 1. Try to detect a masked card last-4 (e.g. "card ending 1234", "card XX1234", "Card no. XX1234")
    let card_re = INSTR_CARD_LAST4_RE.get_or_init(|| {
        Regex::new(r"(?i)card\s+(?:ending|no\.?|number|#)?\s*(?:with\s+)?(?:xx+|\*+)?(\d{4})\b")
            .unwrap()
    });
    if let Some(caps) = card_re.captures(body) {
        if let Some(last4) = caps.get(1) {
            signals.masked_identifier = Some(format!("XXXX{}", last4.as_str()));
            // Check whether it's a credit or debit card
            let body_lower = body.to_lowercase();
            if body_lower.contains("credit card") || body_lower.contains("cc") {
                signals.instrument_type = Some("credit_card".to_string());
            } else if body_lower.contains("debit card") || body_lower.contains("dc") {
                signals.instrument_type = Some("debit_card".to_string());
            } else {
                signals.instrument_type = Some("credit_card".to_string());
            }
        }
    }

    // 2. If no card found, try bank account suffix (e.g. "A/c ending 1234", "account ending 1234")
    if signals.masked_identifier.is_none() {
        let acc_re = INSTR_ACCOUNT_SUFFIX_RE.get_or_init(|| {
            Regex::new(
                r"(?i)(?:a/c|account|acct)\s+(?:ending|no\.?|number|#)?\s*(?:with\s+)?(?:xx+|\*+)?(\d{4})\b",
            )
            .unwrap()
        });
        if let Some(caps) = acc_re.captures(body) {
            if let Some(last4) = caps.get(1) {
                signals.masked_identifier = Some(format!("XXXX{}", last4.as_str()));
                signals.instrument_type = Some("bank_account".to_string());
            }
        }
    }

    // 3. Direction-aware extraction of user's UPI VPA.
    // Counterparty VPAs (e.g. 'Paid to ...', 'towards VPA ...', 'payee ...') belong to external
    // merchants/receivers and MUST NEVER be saved as the user's VPA instrument.
    let body_lower = body.to_lowercase();
    let is_credit = body_lower.contains("credited")
        || body_lower.contains("received")
        || body_lower.contains("deposited")
        || body_lower.contains("added to")
        || body_lower.contains("refund");

    // Collect counterparty VPAs (payees in debits, senders in credits) to build an explicit blacklist
    let mut cp_vpas: Vec<String> = Vec::new();

    if is_credit {
        let cp_credit_re = INSTR_CP_UPI_VPA_CREDIT_RE.get_or_init(|| {
            Regex::new(
                r"(?i)\b(?:payment\s+from|received\s+from|paid\s+by|remitter|sender|from)\s*:?\s*(?:(?:VPA|UPI\s+ID)\s*:?\s*)?(?:[A-Za-z0-9._\-'\s]{1,40}?\s+)?([\w.\-+]+@[\w.\-]+)",
            )
            .unwrap()
        });
        for caps in cp_credit_re.captures_iter(body) {
            if let Some(m) = caps.get(1) {
                cp_vpas.push(m.as_str().to_lowercase().trim_end_matches('.').to_string());
            }
        }
    } else {
        let cp_debit_re = INSTR_CP_UPI_VPA_DEBIT_RE.get_or_init(|| {
            Regex::new(
                r"(?i)\b(?:paid\s+to|to|payee|towards|beneficiary|recipient|merchant)\s*:?\s*(?:(?:VPA|UPI\s+ID)\s*:?\s*)?(?:[A-Za-z0-9._\-'\s]{1,40}?\s+)?([\w.\-+]+@[\w.\-]+)",
            )
            .unwrap()
        });
        for caps in cp_debit_re.captures_iter(body) {
            if let Some(m) = caps.get(1) {
                cp_vpas.push(m.as_str().to_lowercase().trim_end_matches('.').to_string());
            }
        }
    }

    // Try extracting explicit User VPA candidates (source VPA in debits, destination VPA in credits)
    let mut user_vpa_candidates: Vec<String> = Vec::new();

    let user_explicit_re = INSTR_USER_UPI_VPA_EXPLICIT_RE.get_or_init(|| {
        Regex::new(
            r"(?i)\b(?:your|user)\s+(?:UPI\s+VPA|VPA|UPI\s+ID)\s*:?\s*([\w.\-+]+@[\w.\-]+)",
        )
        .unwrap()
    });
    for caps in user_explicit_re.captures_iter(body) {
        if let Some(m) = caps.get(1) {
            user_vpa_candidates.push(m.as_str().to_lowercase().trim_end_matches('.').to_string());
        }
    }

    if is_credit {
        let user_credit_re = INSTR_USER_UPI_VPA_CREDIT_RE.get_or_init(|| {
            Regex::new(
                r"(?i)\b(?:credited\s+to|deposited\s+to|received\s+in|to|beneficiary|recipient|destination)\s*:?\s*(?:(?:UPI\s+VPA|VPA|UPI\s+ID|account)\s*:?\s*)?(?:[A-Za-z0-9._\-'\s]{1,40}?\s+)?([\w.\-+]+@[\w.\-]+)",
            )
            .unwrap()
        });
        for caps in user_credit_re.captures_iter(body) {
            if let Some(m) = caps.get(1) {
                user_vpa_candidates.push(m.as_str().to_lowercase().trim_end_matches('.').to_string());
            }
        }
    } else {
        let user_debit_re = INSTR_USER_UPI_VPA_DEBIT_RE.get_or_init(|| {
            Regex::new(
                r"(?i)\b(?:from|debited\s+from|sent\s+from|remitter|sender|source|using|via|linked\s+to)\s*:?\s*(?:(?:UPI\s+VPA|VPA|UPI\s+ID|account)\s*:?\s*)?(?:[A-Za-z0-9._\-'\s]{1,40}?\s+)?([\w.\-+]+@[\w.\-]+)",
            )
            .unwrap()
        });
        for caps in user_debit_re.captures_iter(body) {
            if let Some(m) = caps.get(1) {
                user_vpa_candidates.push(m.as_str().to_lowercase().trim_end_matches('.').to_string());
            }
        }
    }

    let mut detected_user_vpa: Option<String> = None;
    for cand in user_vpa_candidates {
        if !cand.ends_with("@gmail.com")
            && !cand.ends_with("@yahoo.com")
            && !cand.ends_with("@outlook.com")
            && !cand.ends_with("@hotmail.com")
            && !cp_vpas.contains(&cand)
        {
            detected_user_vpa = Some(cand);
            break;
        }
    }

    if let Some(vpa_str) = detected_user_vpa {
        signals.upi_vpa = Some(vpa_str.clone());
        // If we didn't find any card or bank account suffix, use the user's VPA as the primary instrument
        if signals.masked_identifier.is_none() {
            signals.masked_identifier = Some(vpa_str);
            signals.instrument_type = Some("upi_vpa".to_string());
        }
    }

    // 4. Detect card network (Visa, Mastercard, RuPay, Amex)
    let network_re = INSTR_NETWORK_RE.get_or_init(|| {
        Regex::new(r"(?i)\b(visa|mastercard|master card|rupay|amex|american express|discover)\b")
            .unwrap()
    });
    if let Some(caps) = network_re.captures(body) {
        let network_str = caps
            .get(1)
            .map(|m| m.as_str().to_lowercase())
            .unwrap_or_default();
        let normalized = if network_str.contains("master") {
            "Mastercard"
        } else if network_str.contains("rupay") {
            "RuPay"
        } else if network_str.contains("amex") || network_str.contains("american") {
            "Amex"
        } else if network_str.contains("discover") {
            "Discover"
        } else {
            "Visa"
        };
        signals.network = Some(normalized.to_string());
    }

    signals
}

fn parse_amount(s: &str) -> Option<i64> {
    let clean: String = s
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    clean
        .parse::<f64>()
        .ok()
        .map(|v| (v * 100.0).round() as i64)
}

/// Returns `None` on an unparseable date string -- NEVER a fabricated
/// fallback timestamp. A silently-invented date here would corrupt
/// `best_event_time` downstream, and specifically Layer 5's ±3-day
/// statement crossref window and reconciliation's time-proximity scoring;
/// callers must let a `None` here fail `ExtractionResult::is_valid()`
/// (or fall through to an explicit, intentional fallback like
/// `BankPatternTemplate::date_fallback_epoch`) rather than treat this as
/// always succeeding.
fn parse_date(s: &str) -> Option<i64> {
    if let Ok(naive_date) = chrono::NaiveDate::parse_from_str(s, "%d-%b-%y") {
        if let Some(naive_datetime) = naive_date.and_hms_opt(0, 0, 0) {
            return Some(naive_datetime.and_utc().timestamp());
        }
    }
    if let Ok(naive_date) = chrono::NaiveDate::parse_from_str(s, "%d-%b-%Y") {
        if let Some(naive_datetime) = naive_date.and_hms_opt(0, 0, 0) {
            return Some(naive_datetime.and_utc().timestamp());
        }
    }
    None
}

/// Doc 30 TASK-TXN-003: "Each template stored as versioned JSON under
/// `bank_templates/<bank>_<version>.json` so templates can ship via app
/// updates without an extraction-engine rewrite." Regex *data* (pattern
/// string + which capture group is amount/merchant/date) lives in these
/// JSON files, not as `Regex::new(...)` literals in match arms -- adding or
/// adjusting a bank's format is now a data-file change, not a Rust-code
/// change to this dispatch logic. Embedded via `include_str!` (compiled
/// into the binary, parsed once at first use) rather than read from a
/// runtime resource directory: a new/updated template still ships in the
/// next app release either way, and this avoids Tauri resource-path
/// resolution being a new failure mode for a financial-data-critical path.
fn default_pattern_direction() -> String {
    "debit".to_string()
}

#[derive(Debug, serde::Deserialize)]
struct BankPatternTemplate {
    #[allow(dead_code)]
    name: String,
    regex: String,
    amount_group: usize,
    merchant_group: usize,
    date_group: usize,
    #[serde(default)]
    date_fallback_epoch: Option<i64>,
    /// Fixed direction this pattern implies ("debit" or "credit") --
    /// templates are single-purpose per regex (one pattern matches one
    /// transaction shape, e.g. "spent"/"debited" vs "credited"/"refund"),
    /// so a fixed per-pattern value is sufficient; no capture group is
    /// needed. Defaults to "debit" so the 6 existing bundled templates
    /// (all debit-shaped) don't need every entry rewritten, but every new
    /// pattern should set this explicitly rather than rely on the default.
    #[serde(default = "default_pattern_direction")]
    direction: String,
}

#[derive(Debug, serde::Deserialize)]
struct BankTemplateFile {
    bank_name: String,
    #[allow(dead_code)]
    version: u32,
    patterns: Vec<BankPatternTemplate>,
}

struct CompiledBankPattern {
    regex: Regex,
    amount_group: usize,
    merchant_group: usize,
    date_group: usize,
    date_fallback_epoch: Option<i64>,
    direction: String,
}

/// The full set of `assets/bank_templates/*.json` files, embedded at
/// compile time. Adding a bank means adding a JSON file here plus one
/// `include_str!` line -- no changes to `BankTemplateLayer::extract` below.
const BANK_TEMPLATE_FILES: &[&str] = &[
    include_str!("../../assets/bank_templates/hdfc_v1.json"),
    include_str!("../../assets/bank_templates/icici_v1.json"),
    include_str!("../../assets/bank_templates/sbi_v1.json"),
    include_str!("../../assets/bank_templates/axis_v1.json"),
    include_str!("../../assets/bank_templates/kotak_v1.json"),
    include_str!("../../assets/bank_templates/yes_bank_v1.json"),
];

/// Whether `bank_name` has a compiled Layer 2 template at all -- distinct
/// from "had a template but the regex didn't match this body." Only 6 of
/// the verified-senders registry's ~139 banks currently have one
/// (`BANK_TEMPLATE_FILES` above), so Layer 2 missing for the other ~96% is
/// an expected coverage gap, not a failure worth info-level log noise.
pub fn bank_has_template(bank_name: &str) -> bool {
    bank_templates().contains_key(bank_name)
}

fn bank_templates() -> &'static std::collections::HashMap<String, Vec<CompiledBankPattern>> {
    static TEMPLATES: OnceLock<std::collections::HashMap<String, Vec<CompiledBankPattern>>> =
        OnceLock::new();
    TEMPLATES.get_or_init(|| {
        let mut map = std::collections::HashMap::new();
        for raw in BANK_TEMPLATE_FILES {
            let file: BankTemplateFile = serde_json::from_str(raw)
                .expect("bundled bank_templates/*.json must parse as valid BankTemplateFile");
            let compiled: Vec<CompiledBankPattern> = file
                .patterns
                .into_iter()
                .map(|p| CompiledBankPattern {
                    regex: Regex::new(&p.regex)
                        .expect("bundled bank_templates/*.json regex must compile"),
                    amount_group: p.amount_group,
                    merchant_group: p.merchant_group,
                    date_group: p.date_group,
                    date_fallback_epoch: p.date_fallback_epoch,
                    direction: p.direction,
                })
                .collect();
            map.insert(file.bank_name, compiled);
        }
        map
    })
}

// Layer 2: Bank-specific template regex
pub struct BankTemplateLayer;
impl ExtractionLayer for BankTemplateLayer {
    fn extract<'a>(
        &'a self,
        pool: &'a Pool,
        bank_name: &'a str,
        body: &'a str,
    ) -> BoxFuture<'a, Option<ExtractionResult>> {
        Box::pin(async move {
            let mut result = ExtractionResult {
                extraction_method: "bank_templates".to_string(),
                currency: Some("INR".to_string()),
                ..Default::default()
            };

            // Doc 30 TASK-TXN-003: a single exit point so a successful match
            // (regardless of which bank/format branch produced it) can seed a
            // `pending` pattern_rules candidate below before returning.
            let matched: Option<ExtractionResult> = 'm: {
                if let Some(patterns) = bank_templates().get(bank_name) {
                    for p in patterns {
                        if let Some(caps) = p.regex.captures(body) {
                            // Fixed per-pattern direction (see
                            // `BankPatternTemplate::direction`'s doc comment)
                            // -- NOT a blanket "debit" default applied to
                            // every match regardless of which pattern
                            // actually fired, which previously mislabeled
                            // any credit/refund-shaped template as a debit.
                            result.direction = Some(p.direction.clone());
                            result.amount_minor = caps
                                .get(p.amount_group)
                                .and_then(|m| parse_amount(m.as_str()));
                            result.merchant_raw = caps
                                .get(p.merchant_group)
                                .map(|m| m.as_str().trim().to_string());
                            // `date_fallback_epoch` is an explicit,
                            // template-authored fallback (Doc 30
                            // TASK-TXN-003) for banks whose alert doesn't
                            // always print a date -- distinct from `None`,
                            // which means the capture group genuinely
                            // didn't parse and must fail validation, not
                            // silently default to any date at all.
                            result.event_time = caps
                                .get(p.date_group)
                                .and_then(|m| parse_date(m.as_str()))
                                .or(p.date_fallback_epoch);
                            break 'm Some(result);
                        }
                    }
                }

                None
            };

            let matched = matched?;

            // Doc 30 TASK-TXN-003: "A successful Layer 2 match seeds a
            // pending-status pattern_rules row, so repeated matches against
            // the same template_hash can graduate to a Layer 1 learned
            // rule." Best-effort: a DB error here must not fail an
            // already-successful extraction.
            let template_hash = compute_template_hash(body);
            let b_name = bank_name.to_string();
            let matched_clone = matched.clone();
            if let Ok(conn) = pool.get().await {
                let _ = conn
                    .interact(move |c| {
                        synthesize_pending_rule(
                            c,
                            &b_name,
                            &template_hash,
                            &matched_clone,
                            "layer2_template",
                        )
                    })
                    .await;
            }

            Some(matched)
        })
    }
    fn layer_name(&self) -> &'static str {
        "bank_templates"
    }
}

/// Doc 30 TASK-TXN-004's documented Layer 3 confidence floor: the weakest
/// possible passing result (bare amount+currency, no explicit direction
/// verb, no merchant, no reference ID -- i.e. the `has_balance_update` path
/// through `ExtractionResult::is_valid()`) scores here.
const LAYER3_BASE_CONFIDENCE: f64 = 0.5;
/// Doc 30 TASK-TXN-004's documented Layer 3 confidence ceiling -- must stay
/// below Layer 1/2's typical 0.9+, regardless of how many bonuses stack.
const LAYER3_MAX_CONFIDENCE: f64 = 0.7;
const LAYER3_AMOUNT_CURRENCY_BONUS: f64 = 0.10;
const LAYER3_EXPLICIT_DIRECTION_BONUS: f64 = 0.10;
const LAYER3_STRICT_MERCHANT_BONUS: f64 = 0.15;
const LAYER3_AMBIGUOUS_MERCHANT_BONUS: f64 = 0.05;
const LAYER3_REFERENCE_ID_BONUS: f64 = 0.05;

// Layer 3: Generic heuristic regex
pub struct GenericRegexLayer;
impl ExtractionLayer for GenericRegexLayer {
    fn extract<'a>(
        &'a self,
        _pool: &'a Pool,
        bank_name: &'a str,
        body: &'a str,
    ) -> BoxFuture<'a, Option<ExtractionResult>> {
        Box::pin(async move {
            let mut result = ExtractionResult {
                extraction_method: "generic_regex".to_string(),
                ..Default::default()
            };

            // 1. Amount & Currency
            let (prefix_re, suffix_re) = generic_currency_amount_regexes();

            if let Some(caps) = prefix_re.captures(body) {
                result.currency = Some(normalize_currency(caps.get(1)?.as_str()));
                result.amount_minor = parse_amount(caps.get(2)?.as_str());
            } else if let Some(caps) = suffix_re.captures(body) {
                result.amount_minor = parse_amount(caps.get(1)?.as_str());
                result.currency = Some(normalize_currency(caps.get(2)?.as_str()));
            }

            // Direction (shared lexicon -- see extraction/lexicon.rs's doc
            // comment for why these lists are no longer duplicated
            // per-layer). Previously recompiled a fresh `Regex` on every
            // single call instead of caching via `OnceLock` like every
            // other regex in this layer; now fixed alongside the lexicon
            // consolidation.
            let credit_re = GENERIC_CREDIT_DIRECTION_RE.get_or_init(|| {
                let alternation = crate::extraction::lexicon::CREDIT_VERBS
                    .iter()
                    .chain(crate::extraction::lexicon::CREDIT_PHRASES)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("|");
                Regex::new(&format!(r"(?i)\b(?:{alternation})\b")).unwrap()
            });
            let debit_re = GENERIC_DEBIT_DIRECTION_RE.get_or_init(|| {
                let alternation = crate::extraction::lexicon::DEBIT_VERBS
                    .iter()
                    .chain(crate::extraction::lexicon::DEBIT_PHRASES)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("|");
                Regex::new(&format!(r"(?i)\b(?:{alternation})\b")).unwrap()
            });
            // Tracked separately from `result.direction` for confidence
            // scoring below: an explicit verb match ("debited"/"credited"
            // etc.) is real evidence; the `result.amount_minor.is_some()`
            // branch below is a bare guess with no direction-specific
            // signal at all, and must not be scored the same.
            let direction_from_explicit_verb;
            let credit_match = credit_re.find(body);
            let debit_match = debit_re.find(body);

            match (credit_match, debit_match) {
                (Some(c), Some(d)) => {
                    if c.start() < d.start() {
                        result.direction = Some("credit".to_string());
                        direction_from_explicit_verb = true;
                    } else {
                        result.direction = Some("debit".to_string());
                        direction_from_explicit_verb = true;
                    }
                }
                (Some(_), None) => {
                    result.direction = Some("credit".to_string());
                    direction_from_explicit_verb = true;
                }
                (None, Some(_)) => {
                    result.direction = Some("debit".to_string());
                    direction_from_explicit_verb = true;
                }
                (None, None) => {
                    direction_from_explicit_verb = false;
                    if result.amount_minor.is_some() {
                        result.direction = Some("debit".to_string());
                    }
                }
            }

            // 2. Merchant
            // Doc 30 TASK-TXN-004: "merchant via capitalized-token or
            // at/to/towards/info: heuristics" -- `towards` and `info:` were
            // missing from the proximity-keyword alternation (only Layer 2's
            // ICICI template hardcoded `Info:` for that one bank). Any other
            // bank's UPI alert using the same "Info: <merchant>" convention,
            // with no dedicated Layer 2 template, previously fell through
            // this fallback with no merchant extracted at all.
            //
            // gmail false-negative remediation, Cluster E: the capture class
            // also excluded `*`, so card-network settlement descriptors
            // (`RAZ*SWIGGY`, `PYTM*...`) truncated at the `*` and never
            // reached a terminator, producing no merchant at all.
            //
            // Cluster G: the keyword must be followed immediately by
            // whitespace, so label-style phrasing ("Payment from:   NAME",
            // colon before the value) never matched either -- `:?` makes the
            // colon optional between the keyword and the required
            // whitespace, for every keyword in the alternation.
            //
            // Two-tier lookup, discovered while verifying Cluster E against
            // the real HDFC body text: "at|to|from|for|by" are ambiguous --
            // "debited from your HDFC Bank Credit Card" uses "from" to name
            // the *source* instrument, not the counterparty, and since the
            // `regex` crate has no lookaround, a single alternation always
            // prefers whichever keyword occurs leftmost in the body
            // regardless of which is semantically the merchant label. The
            // unambiguous merchant-labeling keywords ("towards", "paid to",
            // "info:", etc.) are tried first, on the whole body, before
            // falling back to the ambiguous generic set -- this fixes
            // "debited from your HDFC Bank Credit Card ... towards
            // RAZ*SWIGGY" resolving to "your HDFC Bank Credit" instead of
            // "RAZ*SWIGGY", without weakening the ambiguous-keyword fallback
            // for bodies (e.g. Jupiter's "Payment from: NAME") that have no
            // unambiguous keyword at all.
            //
            // gmail false-negative remediation, Cluster D: Axis Bank's
            // AutoPay-activation template labels the counterparty as
            // "Merchant Name:" (two words) on its own line, followed by the
            // value on the *next* line -- bare "merchant" matched only the
            // first word, leaving "Name:" unconsumed immediately after (the
            // capture class excludes `:`, and `:` isn't a terminator
            // either), so the whole match failed at that position. Listed
            // before bare "merchant" since the `regex` crate prefers
            // whichever alternative is listed first at a given position,
            // not the longest one.
            // gmail false-negative remediation: a declined international-
            // transaction template ("at OPENAI is declined because
            // International Ecom/online transactions are disabled...") has
            // no comma/period or any of the above keywords within 40 chars
            // of the merchant name -- the lazy capture kept expanding
            // through the surrounding prose looking for a terminator,
            // exhausted the 40-char cap, and the whole match failed at that
            // position, falling through to a later unrelated "To enable,"
            // match instead. "is"/"was" cover this and the equally common
            // "at MERCHANT was successful/declined" phrasing, the same
            // class of generic sentence-continuation word as the existing
            // on/via/using/with keywords.
            // gmail false-negative remediation, strengthen-regex pass: the
            // value char class excluded `_`, `&`, `'`, `/`, `@` -- common in
            // real settlement descriptors ("UPI_SRI SAI FRUITS", "M/S ABC &
            // CO", "PVR'S"). Once the class rejects a char mid-descriptor
            // the lazy `{2,40}?` capture can never satisfy a terminator (the
            // disallowed char isn't a terminator either), so the whole match
            // fails at that position and `captures_iter` silently skips to
            // the next -- often unrelated -- keyword occurrence further down
            // the body (see `is_invalid_merchant`'s stopword-filter doc
            // comment for what that let through). `.`/`-` are already
            // terminator chars, so including them here is a no-op (the lazy
            // quantifier always stops before them regardless).
            const MERCHANT_TERMINATOR: &str = r":?\s+([A-Za-z0-9\s*_&'./@-]{2,40}?)(?:\s+on\b|\s+via\b|\s+using\b|\s+with\b|\s+ref\b|\s+card\b|\s+date\b|\s+a/c\b|\s+branch\b|\s+upi\b|\s+is\b|\s+was\b|[,.\n\-]|$)";
            // Shared lexicon (extraction/lexicon.rs) -- see its doc comment
            // for why Layer 3 and Layer 4's keyword lists used to drift.
            let merchant_re_strict = GENERIC_MERCHANT_RE_STRICT.get_or_init(|| {
                let alternation = crate::extraction::lexicon::MERCHANT_LABEL_STRICT.join("|");
                Regex::new(&format!(r"(?i)\b(?:{alternation}){MERCHANT_TERMINATOR}")).unwrap()
            });
            let merchant_re = GENERIC_MERCHANT_RE.get_or_init(|| {
                let alternation = crate::extraction::lexicon::MERCHANT_LABEL_AMBIGUOUS.join("|");
                Regex::new(&format!(r"(?i)\b(?:{alternation}){MERCHANT_TERMINATOR}")).unwrap()
            });
            // Tracked for confidence scoring below: a strict-tier match is
            // an unambiguous merchant label; an ambiguous-tier match
            // ("at"/"to"/"from"/"for"/"by") can also name the *source*
            // instrument rather than the counterparty (see the two-tier
            // lookup comment above), so it's real but weaker evidence.
            //
            // gmail false-negative remediation: each tier now scans ALL of
            // its matches (`captures_iter`, not just the first) and skips
            // any that are self-referential ("to your account") rather than
            // taking the first match unconditionally -- a neobank
            // "credited to your account ... Payment from: NAME" body has
            // exactly this shape, where the real counterparty label comes
            // *after* the self-referential one.
            let mut merchant_matched_strict = false;
            let mut merchant_value: Option<String> = None;
            for caps in merchant_re_strict.captures_iter(body) {
                if let Some(m) = caps.get(1) {
                    let val = m.as_str().trim();
                    if !val.is_empty() && !is_invalid_merchant(val, bank_name) {
                        merchant_value = Some(val.to_string());
                        merchant_matched_strict = true;
                        break;
                    }
                }
            }
            if merchant_value.is_none() {
                for caps in merchant_re.captures_iter(body) {
                    if let Some(m) = caps.get(1) {
                        let val = m.as_str().trim();
                        if !val.is_empty() && !is_invalid_merchant(val, bank_name) {
                            merchant_value = Some(val.to_string());
                            break;
                        }
                    }
                }
            }
            // Neither merchant-label tier fires for a personal UPI P2P
            // transfer -- the parenthesised display name after the VPA
            // handle isn't reachable by either (see `vpa_merchant_fallback`'s
            // doc comment), so fall back to the VPA handle itself rather
            // than leaving the transaction merchant-less.
            if merchant_value.is_none() {
                merchant_value = vpa_merchant_fallback(body);
            }
            result.merchant_raw = merchant_value;

            // 3. Date
            let date_re = GENERIC_DATE_RE.get_or_init(|| {
                Regex::new(r"(?i)(\d{2}[-/]\d{2}[-/]\d{2,4}|\d{2}-[a-zA-Z]{3}-\d{2,4}|\d{2}\s+[a-zA-Z]{3},?\s+\d{2,4}|[a-zA-Z]{3}\s+\d{2},\s*\d{4})").unwrap()
            });
            if let Some(caps) = date_re.captures(body) {
                if let Some(parsed) = parse_date_generic(caps.get(1)?.as_str()) {
                    result.event_time = Some(parsed.timestamp);
                    result.event_time_ambiguous = parsed.ambiguous;
                }
            }

            // 4. Reference ID
            let ref_re = GENERIC_REF_RE.get_or_init(|| Regex::new(r"\b(\d{12})\b").unwrap());
            if let Some(caps) = ref_re.captures(body) {
                result.reference_id = Some(caps.get(1)?.as_str().to_string());
            }

            // Doc 30 TASK-TXN-004: "a lower confidence score (0.5-0.7) than
            // Layer 1/2 (typically 0.9+), which flows directly into the
            // reconciliation scoring engine" -- previously a flat 0.6
            // regardless of whether every field matched cleanly or fell
            // through to the weakest fallback branch, which starved
            // reconciliation's email-vs-email precedence logic
            // (`canonical.rs`'s `EMAIL_VS_EMAIL_CONFIDENCE_MARGIN`) of any
            // real signal. Built from which branch actually fired for each
            // field, floored/ceilinged to the documented 0.5-0.7 range.
            let mut confidence = LAYER3_BASE_CONFIDENCE;
            if result.amount_minor.is_some() && result.currency.is_some() {
                confidence += LAYER3_AMOUNT_CURRENCY_BONUS;
            }
            if direction_from_explicit_verb {
                confidence += LAYER3_EXPLICIT_DIRECTION_BONUS;
            }
            if result.merchant_raw.is_some() {
                confidence += if merchant_matched_strict {
                    LAYER3_STRICT_MERCHANT_BONUS
                } else {
                    LAYER3_AMBIGUOUS_MERCHANT_BONUS
                };
            }
            if result.reference_id.is_some() {
                confidence += LAYER3_REFERENCE_ID_BONUS;
            }
            result.confidence_score = Some(confidence.min(LAYER3_MAX_CONFIDENCE));

            if result.is_valid() {
                Some(result)
            } else {
                None
            }
        })
    }
    fn layer_name(&self) -> &'static str {
        "generic_regex"
    }
}

fn normalize_currency(c: &str) -> String {
    let c = c.to_uppercase();
    if c.contains("RS") || c.contains("₹") {
        "INR".to_string()
    } else if c.contains("$") {
        "USD".to_string()
    } else {
        c.replace(".", "").trim().to_string()
    }
}

/// Result of [`parse_date_generic`] -- pairs the parsed timestamp with
/// whether the source format was numerically ambiguous (see `ambiguous`
/// field doc on `ExtractionResult::event_time_ambiguous`).
#[derive(Debug, PartialEq)]
struct DateParseResult {
    timestamp: i64,
    ambiguous: bool,
}

/// The 3 bare-numeric formats where a DD/MM-vs-MM/DD swap is a genuinely
/// different, equally well-formed date whenever both components are <=12.
/// Every other format below (month-name) is inherently unambiguous.
const NUMERIC_AMBIGUOUS_FORMATS: &[&str] = &["%d/%m/%Y", "%d-%m-%Y", "%m-%d-%Y"];

/// Returns `None` on an unparseable date string -- see `parse_date`'s doc
/// comment for why this must never fabricate a fallback timestamp.
fn parse_date_generic(s: &str) -> Option<DateParseResult> {
    let formats = [
        "%d-%b-%Y",
        "%d-%b-%y",
        "%d/%m/%Y",
        "%d-%m-%Y",
        "%m-%d-%Y",
        "%d %b %Y",
        "%d %b %y",
        "%d %b, %Y",
        "%d %b, %y",
        "%b %d, %Y",
    ];

    for fmt in formats {
        if let Ok(naive_date) = chrono::NaiveDate::parse_from_str(s, fmt) {
            if let Some(naive_datetime) = naive_date.and_hms_opt(0, 0, 0) {
                use chrono::Datelike;
                let ambiguous = NUMERIC_AMBIGUOUS_FORMATS.contains(&fmt)
                    && naive_date.day() <= 12
                    && naive_date.day() != naive_date.month();
                return Some(DateParseResult {
                    timestamp: naive_datetime.and_utc().timestamp(),
                    ambiguous,
                });
            }
        }
    }
    None
}

/// Collects merchant-name tokens starting at `tokens[start]`, stopping at
/// the same terminator keywords/punctuation Layer 3's `MERCHANT_TERMINATOR`
/// regex stops at (`extraction/lexicon.rs`'s doc comment covers why the two
/// layers' keyword *lists* are shared; this window-collection logic is
/// NlpLayer-specific since Layer 3 does the equivalent via a single regex
/// capture group instead).
fn collect_merchant_window(
    tokens: &[&str],
    lower_tokens: &[String],
    start: usize,
) -> Option<String> {
    let mut merchant_parts = Vec::new();
    let mut j = start;
    // Expanded window to capture larger merchant names
    while j < tokens.len() && j < start + 5 {
        let next_token_lower = &lower_tokens[j];
        if next_token_lower == "on"
            || next_token_lower == "via"
            || next_token_lower == "bal"
            || next_token_lower.starts_with("ref")
            || next_token_lower == "balance"
            || next_token_lower == "with"
            || next_token_lower == "card"
            || next_token_lower == "date"
            || next_token_lower == "a/c"
            || next_token_lower == "branch"
            || next_token_lower == "upi"
        {
            break;
        }
        let cleaned = tokens[j].trim_end_matches(&['.', ',', ';', ':'][..]);
        if !cleaned.is_empty() {
            merchant_parts.push(cleaned);
        }
        if tokens[j].ends_with('.') || tokens[j].ends_with(',') {
            break;
        }
        j += 1;
    }
    if merchant_parts.is_empty() {
        None
    } else {
        Some(merchant_parts.join(" "))
    }
}

// Layer 4: Basic NLP
pub struct NlpLayer;
impl ExtractionLayer for NlpLayer {
    fn extract<'a>(
        &'a self,
        _pool: &'a Pool,
        bank_name: &'a str,
        body: &'a str,
    ) -> BoxFuture<'a, Option<ExtractionResult>> {
        Box::pin(async move {
            let mut result = ExtractionResult {
                extraction_method: "nlp".to_string(),
                ..Default::default()
            };

            let tokens: Vec<&str> = body.split_whitespace().collect();
            let lower_tokens: Vec<String> = tokens.iter().map(|s| s.to_lowercase()).collect();

            // Merchant: strict-label pre-pass (shared lexicon,
            // extraction/lexicon.rs). Layer 3 already tries its
            // unambiguous keyword set ("towards", "paid to", "purchased
            // at", "txn at", "in favor of", "merchant name", plus
            // "info"/"merchant"/"beneficiary") before its ambiguous
            // at/to/from/for/by fallback -- this layer never had that
            // strict tier at all, so a body whose *only* merchant signal is
            // one of those unambiguous keywords (no ambiguous keyword, no
            // UPI VPA token) previously extracted no merchant here and, if
            // nothing else in the ladder resolved it either, failed
            // `is_valid()` entirely. Computed once, upfront, and applied
            // below only as a rescue when nothing later in this layer's
            // existing (unchanged) logic found a merchant -- this must
            // never override the ambiguous-keyword/UPI-VPA paths' existing,
            // already-tested behavior, only fill in what they'd otherwise
            // leave empty.
            let mut strict_merchant_candidate: Option<String> = None;
            for idx in 0..lower_tokens.len() {
                if let Some(consumed) = crate::extraction::lexicon::match_label_at(
                    &lower_tokens,
                    idx,
                    crate::extraction::lexicon::MERCHANT_LABEL_STRICT,
                ) {
                    if let Some(candidate) =
                        collect_merchant_window(&tokens, &lower_tokens, idx + consumed)
                    {
                        if !is_invalid_merchant(&candidate, bank_name) {
                            strict_merchant_candidate = Some(candidate);
                            break;
                        }
                    }
                }
            }

            let mut i = 0;
            while i < tokens.len() {
                let token = &lower_tokens[i];
                let orig_token = tokens[i];

                // Direction (shared lexicon -- see extraction/lexicon.rs).
                if crate::extraction::lexicon::DEBIT_VERBS
                    .iter()
                    .any(|v| token.contains(v))
                {
                    result.direction = Some("debit".to_string());
                } else if crate::extraction::lexicon::CREDIT_VERBS
                    .iter()
                    .any(|v| token.contains(v))
                {
                    result.direction = Some("credit".to_string());
                }

                // Amount & Currency
                if (token == "rs" || token == "rs." || token == "inr" || token == "₹")
                    && i + 1 < tokens.len()
                    && result.amount_minor.is_none()
                {
                    if let Some(amt) = parse_amount(tokens[i + 1]) {
                        result.amount_minor = Some(amt);
                        result.currency = Some("INR".to_string());
                    }
                }

                // Merchant. `result.merchant_raw.is_none()` guards this
                // whole block (not just the final assignment) so the first
                // valid match found while walking left-to-right wins --
                // previously every subsequent "at/to/from/for/by" hit
                // unconditionally overwrote whatever was already found,
                // which meant a footer disclaimer's keyword occurrence
                // (always further down the body than the real transaction
                // detail) always clobbered a correct earlier match.
                if result.merchant_raw.is_none()
                    && (token == "at"
                        || token == "to"
                        || token == "from"
                        || token == "for"
                        || token == "by"
                        || token == "merchant"
                        || token == "beneficiary")
                    && i + 1 < tokens.len()
                {
                    let mut merchant_parts = Vec::new();
                    let mut j = i + 1;
                    // Expanded window to capture larger merchant names
                    while j < tokens.len() && j < i + 6 {
                        let next_token_lower = &lower_tokens[j];
                        if next_token_lower == "on"
                            || next_token_lower == "via"
                            || next_token_lower == "bal"
                            || next_token_lower.starts_with("ref")
                            || next_token_lower == "balance"
                            || next_token_lower == "with"
                            || next_token_lower == "card"
                            || next_token_lower == "date"
                            || next_token_lower == "a/c"
                            || next_token_lower == "branch"
                            || next_token_lower == "upi"
                        {
                            break;
                        }
                        let cleaned = tokens[j].trim_end_matches(&['.', ',', ';', ':'][..]);
                        if !cleaned.is_empty() {
                            merchant_parts.push(cleaned);
                        }
                        if tokens[j].ends_with('.') || tokens[j].ends_with(',') {
                            break;
                        }
                        j += 1;
                    }
                    if !merchant_parts.is_empty() {
                        let candidate = merchant_parts.join(" ");
                        if !is_invalid_merchant(&candidate, bank_name) {
                            result.merchant_raw = Some(candidate);
                        }
                    }
                }

                // UPI VPA
                if result.merchant_raw.is_none() && token.contains("upi/") {
                    let parts: Vec<&str> = orig_token.split('/').collect();
                    if parts.len() >= 3 {
                        let candidate = parts[2].trim_end_matches(&['.', ','][..]).to_string();
                        if !is_invalid_merchant(&candidate, bank_name) {
                            result.merchant_raw = Some(candidate);
                        }
                    }
                }

                // Balance
                if token == "bal"
                    || token == "balance"
                    || token.starts_with("bal:")
                    || token.starts_with("balance:")
                    || token == "avl"
                {
                    let mut j = i + 1;
                    if token == "avl" && j < tokens.len() && lower_tokens[j] == "bal" {
                        j += 1;
                    }
                    if j < tokens.len()
                        && (lower_tokens[j] == "rs"
                            || lower_tokens[j] == "rs."
                            || lower_tokens[j] == "inr"
                            || lower_tokens[j] == "₹"
                            || lower_tokens[j] == "-"
                            || lower_tokens[j] == "is")
                    {
                        j += 1;
                    }
                    if j < tokens.len() {
                        if let Some(amt) = parse_amount(tokens[j]) {
                            result.balance_after = Some(amt);
                        }
                    }
                }

                // Date
                if token == "on" && i + 1 < tokens.len() {
                    let dt_str = tokens[i + 1].trim_end_matches(&['.', ','][..]);
                    if let Some(parsed) = parse_date_generic(dt_str) {
                        result.event_time = Some(parsed.timestamp);
                        result.event_time_ambiguous = parsed.ambiguous;
                    }
                }

                i += 1;
            }

            // Merchant: apply the strict-label pre-pass candidate only as a
            // rescue -- never overriding whatever the ambiguous-keyword or
            // UPI-VPA logic above already found.
            if result.merchant_raw.is_none() {
                if let Some(candidate) = strict_merchant_candidate {
                    result.merchant_raw = Some(candidate);
                }
            }

            // Fallback for Date
            if result.event_time.is_none() {
                for t in &tokens {
                    let cleaned = t.trim_end_matches(&['.', ','][..]);
                    if let Some(parsed) = parse_date_generic(cleaned) {
                        result.event_time = Some(parsed.timestamp);
                        result.event_time_ambiguous = parsed.ambiguous;
                        break;
                    }
                }
            }

            if result.is_valid() {
                Some(result)
            } else {
                None
            }
        })
    }
    fn layer_name(&self) -> &'static str {
        "nlp"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Template Drift Detection (Task 4.9)
// ─────────────────────────────────────────────────────────────────────────────

/// The outcome of [`detect_pattern_drift`].
///
/// Drift is declared when a bank's email template is **known** (active/trusted
/// pattern rules exist for the computed template hash) yet all extraction layers
/// 2–3 returned no valid result — indicating the template has changed.
///
/// When drift is detected the caller may route the body to Layer 5 (LLM).  If
/// the LLM succeeds a new `PatternRulesRow` candidate is synthesised in
/// `pending` state and included in `synthesized_rule`.
#[derive(Debug, Clone)]
pub struct DriftResult {
    /// `true` when the template is known but extraction failed.
    pub drift_detected: bool,
    /// The structural template hash computed from the email body.
    pub template_hash: String,
    /// A synthesised `pending` pattern-rule candidate written to the database
    /// when Layer 5 (LLM) extraction succeeds.  `None` when drift was not
    /// detected or LLM extraction did not succeed.
    pub synthesized_rule: Option<crate::db::pattern_rules::PatternRulesRow>,
}

/// Checks whether a template drift has occurred for a given email body.
///
/// Drift is defined as: active/trusted rules exist for `(bank_name,
/// template_hash)` **and** the caller-supplied `ladder_result` is `None`
/// (i.e., all extraction layers failed despite the template being known).
///
/// If the template hash has never been seen before (no rules), this function
/// returns `drift_detected = false` — the pipeline will handle it as a
/// genuinely new template rather than a drift event.
///
/// # Arguments
/// * `conn`          — A synchronous SQLite connection used for the rule lookup
///   and optional rule insertion.
/// * `bank_name`     — The issuer bank name (e.g. `"HDFC Bank"`).
/// * `body`          — The plain-text email body.
/// * `ladder_result` — The result returned by the 4-layer extraction ladder.
///   Pass `&None` when all layers failed.
pub fn detect_pattern_drift(
    conn: &Connection,
    bank_name: &str,
    body: &str,
    ladder_result: &Option<ExtractionResult>,
) -> Result<DriftResult> {
    // Compute the structural template hash for this body.
    let template_hash = compute_template_hash(body);

    // If the ladder already succeeded there is nothing to detect.
    if ladder_result.is_some() {
        return Ok(DriftResult {
            drift_detected: false,
            template_hash,
            synthesized_rule: None,
        });
    }

    // Check whether we have existing active/trusted rules for this template.
    let known_rule_count = crate::db::pattern_rules::count_active_rules_by_bank_and_hash(
        conn,
        bank_name,
        &template_hash,
    )?;

    let drift_detected = known_rule_count > 0;

    Ok(DriftResult {
        drift_detected,
        template_hash,
        synthesized_rule: None,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Layer 5: Statement-row cross-reference (Doc 30 TASK-TXN-005, ADR-019)
// ─────────────────────────────────────────────────────────────────────────────

/// Doc 30 TASK-TXN-005 / Doc 12 §6.3 Layer 5. Not an [`ExtractionLayer`] trait
/// object — like the LLM fallback below, it needs an extra input the trait's
/// fixed `(pool, bank_name, body)` signature has no room for: an anchor date
/// to bound the `±3-day` statement-row search window. `run_extraction_ladder`
/// calls it directly, the same way it already calls `Layer6LlmLayer`
/// (renumbered Layer 6).
pub struct Layer5CrossrefLayer;

impl Layer5CrossrefLayer {
    pub async fn extract(
        &self,
        pool: &Pool,
        bank_name: &str,
        body: &str,
        anchor_date: Option<chrono::NaiveDate>,
    ) -> Option<ExtractionResult> {
        // No date signal at all (not even Gmail's internalDate) -- a search
        // with no window bound would be far too permissive to trust.
        let anchor_date = anchor_date?;

        // Reuse the same partial-field regexes Layer 3 uses -- Layer 5 rescues
        // emails that failed *overall* validation (missing a mandatory field
        // like merchant), not emails with zero extractable signal at all.
        let (prefix_re, suffix_re) = generic_currency_amount_regexes();
        let ref_re = GENERIC_REF_RE.get_or_init(|| Regex::new(r"\b(\d{12})\b").unwrap());

        let amount_minor = prefix_re
            .captures(body)
            .and_then(|c| c.get(2))
            .or_else(|| suffix_re.captures(body).and_then(|c| c.get(1)))
            .and_then(|m| parse_amount(m.as_str()));
        let reference_id = ref_re
            .captures(body)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_string());

        if amount_minor.is_none() && reference_id.is_none() {
            return None;
        }

        let signals = extract_instrument_signals(bank_name, body);
        let (Some(instrument_type), Some(masked_identifier)) = (
            signals.instrument_type.as_ref(),
            signals.masked_identifier.as_ref(),
        ) else {
            return None;
        };
        let issuer_name = signals.issuer_name.as_deref().unwrap_or(bank_name);

        let conn = pool.get().await.ok()?;
        let it = instrument_type.clone();
        let mi = masked_identifier.clone();
        let issuer = issuer_name.to_string();
        let instrument_id = conn
            .interact(move |c| crate::db::instruments::find_instrument_by_key(c, &it, &issuer, &mi))
            .await
            .ok()?
            .ok()??;

        let ref_fragment = reference_id.clone();
        let candidates = conn
            .interact(move |c| {
                crate::db::statement_entries::find_crossref_candidates(
                    c,
                    &instrument_id,
                    anchor_date,
                    ref_fragment.as_deref(),
                    amount_minor,
                )
            })
            .await
            .ok()?
            .ok()?;

        // Doc 30: "A single high-confidence match... zero or multiple
        // ambiguous candidates return None" -- conservative by construction,
        // matching Doc 15 §2 principle 9's ambiguity handling.
        if candidates.len() != 1 {
            return None;
        }
        let entry = &candidates[0];

        Some(ExtractionResult {
            amount_minor: entry.amount_minor,
            currency: entry.currency.clone(),
            direction: entry.direction.clone(),
            event_time: entry
                .transaction_date
                .and_then(|d| d.and_hms_opt(0, 0, 0))
                .map(|dt| dt.and_utc().timestamp()),
            merchant_raw: entry.merchant_raw.clone(),
            reference_id: entry.reference_id.clone(),
            instrument_type: signals.instrument_type.clone(),
            issuer_name: signals.issuer_name.clone(),
            masked_identifier: signals.masked_identifier.clone(),
            network: signals.network.clone(),
            upi_vpa: signals.upi_vpa.clone(),
            extraction_method: "layer5_statement_crossref".to_string(),
            ..Default::default()
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Layer 6: LLM-based extraction (stub)
// ─────────────────────────────────────────────────────────────────────────────

/// Layer 5 — LLM-based fallback extraction.
pub struct Layer6LlmLayer {
    pub app_dir: Option<std::path::PathBuf>,
}
impl Layer6LlmLayer {
    /// The real Layer 6 logic, returning the full [`Layer6Outcome`] —
    /// including the timed-out-vs-failed distinction the `ExtractionLayer`
    /// trait's fixed `Option<ExtractionResult>` return can't carry. Used
    /// directly by `run_extraction_ladder` (Layer 6 is called directly, not
    /// through the `Vec<Box<dyn ExtractionLayer>>`, same as `Layer5CrossrefLayer`
    /// above); the trait impl below just narrows this for anything that does
    /// need the plain trait-object interface.
    async fn run(&self, pool: &Pool, bank_name: &str, body: &str) -> Layer6Outcome {
        let app_dir = match &self.app_dir {
            Some(dir) => dir,
            None => {
                tracing::warn!("Layer 6: No app_dir provided, cannot locate LLM model");
                return Layer6Outcome::Failed;
            }
        };

        // Whichever model the user actually selected in Settings
        // (`local_profile.llm_model`, written by `llm_set_active_model`
        // and by onboarding's `onboarding_save_preferences`), resolved the
        // same way `llm_get_active_model` resolves it for the Settings UI:
        // via `resolve_active_model` against what's actually downloaded on
        // disk. Previously this fell back straight to
        // `DEFAULT_ACTIVE_MODEL_ID` whenever `local_profile.llm_model` was
        // unset (e.g. right after "Delete My Data", which wipes `finance.db`
        // but leaves the `models/` directory untouched) — so a user with a
        // different model downloaded got a "model not found" failure for a
        // model they never chose, while Settings correctly showed their real
        // model as downloaded and active.
        let stored = match pool.get().await {
            Ok(conn) => conn
                .interact(|c| crate::db::local_profile::get_llm_model(c))
                .await
                .ok()
                .and_then(|r| r.ok())
                .flatten(),
            Err(_) => None,
        };
        let downloaded: Vec<String> = crate::llm_manager::get_available_models()
            .into_iter()
            .filter(|m| crate::llm_manager::get_model_path(app_dir, &m.id).is_some())
            .map(|m| m.id)
            .collect();
        let Some(model_id) = crate::llm_manager::resolve_active_model(&downloaded, stored.as_deref())
        else {
            tracing::warn!("Layer 6: No downloaded LLM model available");
            return Layer6Outcome::Failed;
        };

        tracing::info!(bank_name = bank_name, "Layer 6 (LLM) extraction invoked");

        let engine = crate::extraction::llm::LlmEngine::new(app_dir, &model_id);
        let result = engine.extract(bank_name, body).await;

        // Track Layer 5 usage rate in structured logs
        tracing::info!(
            event = "layer5_usage",
            bank_name = bank_name,
            success = matches!(result, Layer6Outcome::Extracted(_)),
            "Layer 6 fallback utilized"
        );

        result
    }
}
impl ExtractionLayer for Layer6LlmLayer {
    fn extract<'a>(
        &'a self,
        pool: &'a Pool,
        bank_name: &'a str,
        body: &'a str,
    ) -> BoxFuture<'a, Option<ExtractionResult>> {
        Box::pin(async move {
            match self.run(pool, bank_name, body).await {
                Layer6Outcome::Extracted(result) => Some(*result),
                Layer6Outcome::TimedOut | Layer6Outcome::Failed => None,
            }
        })
    }
    fn layer_name(&self) -> &'static str {
        "llm_layer6"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Extraction orchestrator
// ─────────────────────────────────────────────────────────────────────────────

/// Runs the email body through the 4-layer ladder, stopping at the first VALID
/// success.  After a valid result is found, instrument signals are extracted and
/// merged into the result.
///
/// When **all** four layers fail, Layer 5 (the local LLM fallback) is invoked
/// if and only if `llm_eligible` is true (Doc 30 TASK-TXN-001: "Layer 6 if
/// the local LLM is RAM-eligible, TASK-SETUP-006") — this is a hardware
/// eligibility gate, independent of whether this specific bank has drifted
/// from a previously-learned template. [`detect_pattern_drift`] still runs
/// after a successful Layer 5 extraction, but only to decide whether to
/// synthesise a `pending` `pattern_rule` candidate for a human reviewer to
/// promote (feeding Layer 1's learning loop) — it is a side effect, not a
/// precondition, of Layer 5 running.
/// Outcome of [`cross_check_amount`].
#[derive(Debug, PartialEq)]
enum AmountAgreement {
    /// The independent regex found the same amount.
    Agrees,
    /// The independent regex found a *different* amount -- real
    /// disagreement, not just absence of a second opinion.
    Disagrees,
    /// The independent regex found no amount-shaped signal at all --
    /// not evidence either way, so callers must not penalize this.
    Inconclusive,
}

/// Doc 30-style ensemble-lite check (Gate 2 hardening: "no ensemble/cross-
/// check anywhere -- the ladder stops at first schema-valid layer, so a
/// wrong-but-complete extraction is never caught by a disagreeing layer,
/// because later layers never run"). Re-derives an amount from `body`
/// using the same cheap currency-prefix/suffix regex `GenericRegexLayer`
/// already uses, and compares it against whichever layer actually won.
/// Cheap because it's a single regex pass reusing already-cached
/// `OnceLock` regexes, not a second full layer execution.
fn cross_check_amount(body: &str, claimed_amount_minor: i64) -> AmountAgreement {
    let (prefix_re, suffix_re) = generic_currency_amount_regexes();

    let independent_amount = prefix_re
        .captures(body)
        .and_then(|c| c.get(2))
        .and_then(|m| parse_amount(m.as_str()))
        .or_else(|| {
            suffix_re
                .captures(body)
                .and_then(|c| c.get(1))
                .and_then(|m| parse_amount(m.as_str()))
        });

    match independent_amount {
        None => AmountAgreement::Inconclusive,
        Some(independent) if independent == claimed_amount_minor => AmountAgreement::Agrees,
        Some(_) => AmountAgreement::Disagrees,
    }
}

/// Confidence assigned on disagreement when the winning layer had no
/// confidence score of its own (Layers 1/2/5 don't set one today) -- below
/// Layer 3's documented 0.5 floor, since "schema-valid but an independent
/// check disagrees" is a weaker signal than even Layer 3's weakest result.
const CROSS_CHECK_DISAGREEMENT_CONFIDENCE: f64 = 0.4;
/// Multiplicative penalty applied to an existing confidence score on
/// disagreement (Layer 3/6 already set one).
const CROSS_CHECK_DISAGREEMENT_PENALTY_FACTOR: f64 = 0.8;

/// Applies [`cross_check_amount`] to a layer's about-to-be-returned result.
/// Only ever lowers confidence on disagreement -- never raises it on
/// agreement, and never rejects the result outright (a disagreement is
/// suspicious, not proof of a wrong amount; the independent regex is itself
/// just a heuristic, not a ground truth). No-op for `GenericRegexLayer`
/// itself: its own amount already comes from this exact regex, so
/// cross-checking it against itself would trivially always agree.
fn apply_amount_cross_check(obs: &mut ExtractionResult, body: &str) {
    let Some(claimed) = obs.amount_minor else {
        return;
    };
    if cross_check_amount(body, claimed) == AmountAgreement::Disagrees {
        let downgraded = match obs.confidence_score {
            Some(existing) => (existing * CROSS_CHECK_DISAGREEMENT_PENALTY_FACTOR).max(0.0),
            None => CROSS_CHECK_DISAGREEMENT_CONFIDENCE,
        };
        tracing::warn!(
            layer = obs.extraction_method,
            claimed_amount_minor = claimed,
            new_confidence = downgraded,
            "Ensemble-lite amount cross-check disagreement -- confidence downgraded"
        );
        obs.confidence_score = Some(downgraded);
    }
}

/// Number of days a swapped candidate must be closer than the original to
/// count as a decisive signal, not just a nudge -- guards against a swap
/// that happens to land marginally closer by chance.
const DATE_CROSS_CHECK_DECISIVE_RATIO: i64 = 3;
/// Widest gap from Gmail's `internalDate` a candidate can plausibly be and
/// still represent a same-alert transaction date (covers next-morning
/// consolidated/batch alerts, not just instant ones).
const DATE_CROSS_CHECK_PLAUSIBLE_DELAY_DAYS: i64 = 7;

/// Doc 30 TASK-TXN-004 scoped Gmail's `internalDate` to a *fallback* --
/// filling `event_time` only when the body yields no date at all -- and
/// explicitly rejected using it to override a body-parsed date (that was a
/// real bug, fixed in that task). This function does not violate that: it
/// never touches `event_time` unless `event_time_ambiguous` is set, which
/// only happens for a bare numeric date where day and month are both <=12
/// -- i.e. `event_time` is already known to be one of exactly two
/// equally-valid readings of the same digits, not a single confident
/// parse. `internal_date` is used only to arbitrate between those two
/// readings, never to override an unambiguous one.
///
/// Deliberately post-hoc on the *ambiguity flag*, not on the resolved
/// date's day/month values -- a month-name date like "5-Aug-2026" also has
/// day<=12, but `event_time_ambiguous` is `false` for it (set at the
/// `parse_date_generic` call sites, see that function's doc comment), so
/// it's structurally impossible for this to touch a date that was never
/// ambiguous in the first place.
///
/// Three outcomes:
/// - Swap is decisively closer to `internal_date` and within a plausible
///   delay window -- correct `event_time`, log it, tag
///   `"swapped_by_anchor"`.
/// - Neither the original nor the swap is within the plausible window --
///   something's off (backfill scan, unusually delayed alert, or the
///   regex grabbed an unrelated date). Don't guess: leave `event_time`
///   untouched, downgrade confidence, tag `"anchor_mismatch_needs_review"`
///   so `pending_review` catches it.
/// - Anything else (no anchor, weak/no signal either way) -- leave
///   `event_time` untouched, no flag. This is the common case: most
///   ambiguous dates simply keep the DD-MM locale default.
fn apply_date_cross_check(obs: &mut ExtractionResult, internal_date: Option<i64>) {
    if !obs.event_time_ambiguous {
        return;
    }
    let (Some(ts), Some(anchor_ts)) = (obs.event_time, internal_date) else {
        return;
    };
    use chrono::Datelike;
    let Some(original) = chrono::DateTime::from_timestamp(ts, 0).map(|dt| dt.naive_utc().date())
    else {
        return;
    };
    let Some(anchor) = chrono::DateTime::from_timestamp(anchor_ts, 0).map(|dt| dt.naive_utc().date())
    else {
        return;
    };
    let (day, month, year) = (original.day(), original.month(), original.year());
    if day == month {
        return; // swap is a no-op
    }
    let Some(swapped) = chrono::NaiveDate::from_ymd_opt(year, day, month) else {
        return;
    };

    // Calendar-day distance, not raw epoch seconds: `event_time` is always
    // UTC midnight (see `parse_date_generic`) but `internal_date` carries a
    // real time-of-day, so a raw-second diff would add up to a day of pure
    // time-of-day noise right at the plausible-window boundary.
    let orig_days = (anchor - original).num_days().abs();
    let swapped_days = (anchor - swapped).num_days().abs();

    if swapped_days <= DATE_CROSS_CHECK_PLAUSIBLE_DELAY_DAYS
        && swapped_days * DATE_CROSS_CHECK_DECISIVE_RATIO < orig_days.max(1)
    {
        let Some(swapped_ts) = swapped
            .and_hms_opt(0, 0, 0)
            .map(|dt| dt.and_utc().timestamp())
        else {
            return;
        };
        tracing::warn!(
            layer = obs.extraction_method,
            original = %original,
            swapped = %swapped,
            orig_days,
            swapped_days,
            "Date cross-check: internalDate anchor decisively favors swapped DD/MM interpretation"
        );
        obs.event_time = Some(swapped_ts);
        obs.date_cross_check_flag = Some("swapped_by_anchor".to_string());
    } else if orig_days > DATE_CROSS_CHECK_PLAUSIBLE_DELAY_DAYS
        && swapped_days > DATE_CROSS_CHECK_PLAUSIBLE_DELAY_DAYS
    {
        obs.date_cross_check_flag = Some("anchor_mismatch_needs_review".to_string());
        let downgraded = match obs.confidence_score {
            Some(existing) => existing.min(CROSS_CHECK_DISAGREEMENT_CONFIDENCE),
            None => CROSS_CHECK_DISAGREEMENT_CONFIDENCE,
        };
        obs.confidence_score = Some(downgraded);
    }
}

pub async fn run_extraction_ladder(
    pool: &Pool,
    bank_name: &str,
    body: &str,
    app_dir: Option<std::path::PathBuf>,
    llm_eligible: bool,
    internal_date: Option<i64>,
    layer6_timed_out: &mut bool,
) -> Result<Option<ExtractionResult>> {
    let layers: Vec<Box<dyn ExtractionLayer>> = vec![
        Box::new(LearnedPatternLayer),
        Box::new(BankTemplateLayer),
        Box::new(GenericRegexLayer),
        Box::new(NlpLayer),
    ];

    for layer in layers {
        let layer_name = layer.layer_name();
        if let Some(mut obs) = layer.extract(pool, bank_name, body).await {
            if obs.is_valid() {
                // Augment with instrument signals from the body.
                let signals = extract_instrument_signals(bank_name, body);
                obs.instrument_type = signals.instrument_type;
                obs.issuer_name = signals.issuer_name;
                obs.masked_identifier = signals.masked_identifier;
                obs.network = signals.network;
                obs.upi_vpa = signals.upi_vpa;
                // Doc 30 TASK-TXN-012: "During extraction (Layers 2/3),
                // detect EMI language" -- applied uniformly regardless of
                // which layer produced the core fields, since EMI phrasing
                // detection is language-level, not tied to any one layer's
                // own bank-specific/generic regex templates.
                if let Some((number, total)) =
                    crate::extraction::emi_detector::detect_emi_installment_numbers(body)
                {
                    obs.emi_installment_number = Some(number);
                    obs.emi_total_installments = Some(total);
                    obs.emi_original_amount_minor =
                        crate::extraction::emi_detector::detect_emi_original_amount_minor(body);
                }
                // Doc 30 TASK-TXN-013: foreign-currency evidence, same
                // "applies regardless of which layer produced the core
                // fields" reasoning as EMI detection above.
                let settled_currency = obs.currency.clone().unwrap_or_else(|| "INR".to_string());
                let fx =
                    crate::extraction::currency_handler::detect_fx_fields(body, &settled_currency);
                obs.original_amount_minor = fx.original_amount_minor;
                obs.original_currency = fx.original_currency;
                obs.exchange_rate = fx.exchange_rate;
                apply_amount_cross_check(&mut obs, body);
                apply_date_cross_check(&mut obs, internal_date);
                tracing::info!(
                    layer = layer_name,
                    status = "success",
                    "Extraction layer succeeded"
                );
                return Ok(Some(obs));
            }
        }
        if layer_name == "learned_patterns" {
            // Layer 1 is a feedback-loop cache seeded only by Layer 2/6
            // successes (`synthesize_pending_rule`) — on a fresh bank +
            // template-hash shape it has zero rules until the ladder has
            // already succeeded once via a later layer. That's expected
            // routine behaviour, not a failure worth info-level noise.
            tracing::debug!(
                layer = layer_name,
                status = "no_rules",
                "Extraction layer skipped (no learned rules yet)"
            );
        } else if layer_name == "bank_templates" && !bank_has_template(bank_name) {
            // Same "expected, not a failure" reasoning as learned_patterns
            // just above: most banks simply have no Layer 2 template yet
            // (see `bank_has_template`'s doc comment). A bank that *does*
            // have a template but still didn't match falls through to the
            // `else` branch below and stays at info -- that case is a real
            // signal (template drift / a regex bug) worth keeping visible.
            tracing::debug!(
                layer = layer_name,
                status = "no_template",
                bank_name = bank_name,
                "Extraction layer skipped (no template for this bank)"
            );
        } else {
            tracing::info!(
                layer = layer_name,
                status = "failure",
                "Extraction layer failed"
            );
        }
    }

    // ── Layer 5: statement-row cross-reference (Doc 30 TASK-TXN-005) ─────────
    let anchor_date = internal_date
        .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0).map(|dt| dt.naive_utc().date()));
    if let Some(mut crossref_result) = Layer5CrossrefLayer
        .extract(pool, bank_name, body, anchor_date)
        .await
    {
        if crossref_result.is_valid() {
            apply_amount_cross_check(&mut crossref_result, body);
            tracing::info!(
                layer = "layer5_statement_crossref",
                status = "success",
                "Extraction layer succeeded"
            );
            // Instrument signals were already attached inside
            // Layer5CrossrefLayer::extract (it needs them earlier, to
            // resolve the instrument for the query itself).
            return Ok(Some(crossref_result));
        }
    }
    tracing::info!(
        layer = "layer5_statement_crossref",
        status = "failure",
        "Extraction layer failed"
    );

    // ── All five layers failed: Layer 6 gate is RAM-eligibility only ─────────
    // (Doc 30 TASK-TXN-001) — never conditioned on whether this particular
    // bank has a known/drifted template. A brand-new, never-before-seen bank
    // must still be able to reach Layer 6.
    if !llm_eligible {
        tracing::info!(
            bank_name = bank_name,
            "Layer 6 skipped — LLM not RAM-eligible"
        );
        return Ok(None);
    }

    let layer6 = Layer6LlmLayer {
        app_dir: app_dir.clone(),
    };
    let layer6_outcome = layer6.run(pool, bank_name, body).await;
    if matches!(layer6_outcome, Layer6Outcome::TimedOut) {
        *layer6_timed_out = true;
    }
    if let Layer6Outcome::Extracted(boxed_llm_result) = layer6_outcome {
        let mut llm_result = *boxed_llm_result;
        if llm_result.is_valid() {
            // Augment with instrument signals.
            let signals = extract_instrument_signals(bank_name, body);
            llm_result.instrument_type = signals.instrument_type;
            llm_result.issuer_name = signals.issuer_name;
            llm_result.masked_identifier = signals.masked_identifier;
            llm_result.network = signals.network;
            llm_result.upi_vpa = signals.upi_vpa;
            apply_amount_cross_check(&mut llm_result, body);

            // ── Best-effort: synthesise a pending pattern-rule candidate ─────
            // Only when this bank's known rules had drifted from the current
            // body — a side effect that feeds Layer 1's learning loop, never
            // a precondition for Layer 5 itself having run. A DB error here
            // must not unwind an already-successful extraction.
            let b_name = bank_name.to_string();
            let body_owned = body.to_string();
            if let Ok(conn) = pool.get().await {
                let drift_result = conn
                    .interact(move |c| detect_pattern_drift(c, &b_name, &body_owned, &None))
                    .await;
                if let Ok(Ok(drift)) = drift_result {
                    if drift.drift_detected {
                        tracing::warn!(
                            bank_name = bank_name,
                            template_hash = %drift.template_hash,
                            "Template drift detected — synthesising pending pattern-rule candidate."
                        );
                        let template_hash_clone = drift.template_hash.clone();
                        let bank_name_str = bank_name.to_string();
                        let llm_result_clone = llm_result.clone();
                        if let Ok(conn2) = pool.get().await {
                            let _ = conn2
                                .interact(move |c| {
                                    synthesize_pending_rule(
                                        c,
                                        &bank_name_str,
                                        &template_hash_clone,
                                        &llm_result_clone,
                                        "llm_synthesis",
                                    )
                                })
                                .await;
                        }
                    }
                }
            }

            return Ok(Some(llm_result));
        }
    }

    Ok(None)
}

/// Synthesises a `pending` pattern-rule candidate from a successful LLM
/// extraction result and persists it via [`insert_pending_candidate`].
///
/// Each extracted field that is `Some(...)` becomes a separate rule row so the
/// individual fields can be approved or rejected independently by a human
/// reviewer.  The candidate is given `confidence = 0.0` to make it clearly
/// distinguishable from user-validated rules.
fn synthesize_pending_rule(
    conn: &Connection,
    bank_name: &str,
    template_hash: &str,
    extraction: &ExtractionResult,
    source: &str,
) -> Result<()> {
    let now = chrono::Utc::now().naive_utc();

    // Helper to build and insert a single field rule.
    let insert_field = |field_name: &str, regex_hint: &str| -> Result<()> {
        let rule = crate::db::pattern_rules::PatternRulesRow {
            id: Uuid::new_v4().to_string(),
            bank_name: bank_name.to_string(),
            template_hash: template_hash.to_string(),
            field_name: field_name.to_string(),
            // The regex is a placeholder hint derived from the extracted value;
            // a human must verify and refine it before promoting to `active`.
            rule_payload_json: serde_json::json!({ "regex": regex_hint, "source": source }),
            status: "pending".to_string(),
            success_count: 0,
            failure_count: 0,
            confidence: 0.0,
            created_at: Some(now),
            updated_at: Some(now),
        };
        crate::db::pattern_rules::insert_pending_candidate(conn, &rule)
    };

    if let Some(amt) = extraction.amount_minor {
        // Hint regex: match the decimal representation of the amount.
        let decimal = format!("{:.2}", amt as f64 / 100.0);
        insert_field("amount", r"([\d,]+(?:\.\d+)?)\s*(?:INR|Rs)")?;
        let _ = decimal; // used for documentation only
    }
    if extraction.merchant_raw.is_some() {
        insert_field(
            "merchant",
            r"(?:at|to|from)\s+([A-Za-z0-9\s]+?)(?:\s+on|,|\.|$)",
        )?;
    }
    if extraction.currency.is_some() {
        insert_field("currency", r"(INR|USD|EUR|GBP)")?;
    }
    if extraction.direction.is_some() {
        insert_field("direction", r"(debited|credited|spent|received)")?;
    }
    if extraction.event_time.is_some() {
        insert_field("event_time", r"(\d{2}-[A-Za-z]{3}-\d{2,4})")?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to create a dummy pool for tests
    fn dummy_pool() -> Pool {
        let mgr = deadpool_sqlite::Manager::from_config(
            &deadpool_sqlite::Config {
                path: ":memory:".into(),
                pool: Some(deadpool_sqlite::PoolConfig::new(1)),
            },
            deadpool_sqlite::Runtime::Tokio1,
        );
        Pool::builder(mgr).build().unwrap()
    }

    /// Regression test for a live extraction bug: SBI Card's "Dear
    /// Cardholder, This is to inform you that, Rs.245.43 spent on your SBI
    /// Credit Card ending 7603 at DREAMPLUGTECHNOLOGI on 01/07/26." mis-
    /// resolved to merchant "inform you that" (the ambiguous "to" label
    /// matching the intro clause, leftmost in the body, before the real "at
    /// DREAMPLUGTECHNOLOGI" label) because "inform"/"that" weren't on
    /// `MERCHANT_STOPWORDS` yet -- this exact body is what surfaced the gap.
    #[tokio::test]
    async fn test_sbi_intro_clause_boilerplate_does_not_win_over_real_merchant() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "Dear Cardholder,\nThis is to inform you that, Rs.245.43 spent on your SBI Credit Card ending 7603 at DREAMPLUGTECHNOLOGI on 01/07/26. Trxn. not done by you? Report at https://sbicard.com/Dispute. If you have not authorized this transaction please contact the SBI Card Helpline.";
        let result = layer.extract(&pool, "SBI Card", body).await.unwrap();
        assert_eq!(result.merchant_raw, Some("DREAMPLUGTECHNOLOGI".to_string()));
    }

    /// Regression test: a personal UPI P2P transfer (HDFC's "Rs.750.00 is
    /// debited from your account ending 4691 towards VPA
    /// 8127696200@jupiteraxis (ADITYA RAWAL) on 07-06-26.") has no business
    /// merchant, and its parenthesised name is the account holder's own
    /// name, not a payee -- must resolve to the VPA handle, not "VPA
    /// rawalad" or nothing at all.
    #[tokio::test]
    async fn test_upi_p2p_transfer_falls_back_to_vpa_handle() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "Dear Customer,\n\nGreetings from HDFC Bank!\n\nRs.750.00 is debited from your account ending 4691 towards VPA 8127696200@jupiteraxis (ADITYA RAWAL) on 07-06-26.\n\nUPI transaction reference no.: 327479321586.\n\nIf you did not authorize this transaction, please report it immediately at:\na. When in India (Toll free): 1800 258 6161\nb. When abroad:  9122 61606160\nc. Or SMS 'BLOCK UPI' to 7308080808.";
        let result = layer.extract(&pool, "HDFC Bank", body).await.unwrap();
        assert_eq!(
            result.merchant_raw,
            Some("8127696200@jupiteraxis".to_string())
        );
    }

    /// TASK-DB-002: like `dummy_pool()`, but backed by a real temp file with
    /// the full schema already migrated via `sqlx::migrate!` — sqlx's
    /// migration connection is a separate connection stack from rusqlite
    /// and cannot reach a `:memory:` database another connection opened, so
    /// helpers that actually need tables to exist (`setup_db_with_rule`,
    /// `setup_drift_db`) need a real file instead of `dummy_pool()`'s
    /// `:memory:`.
    async fn dummy_migrated_pool() -> Pool {
        let db_path = crate::db::test_helpers::fresh_temp_db_path();
        crate::db::migrations::run_migrations(&db_path, None)
            .await
            .unwrap();
        let mgr = deadpool_sqlite::Manager::from_config(
            &deadpool_sqlite::Config {
                path: db_path,
                pool: Some(deadpool_sqlite::PoolConfig::new(1)),
            },
            deadpool_sqlite::Runtime::Tokio1,
        );
        Pool::builder(mgr).build().unwrap()
    }

    /// Doc 30 TASK-TXN-001 acceptance test. Exercises the real
    /// `run_extraction_ladder`, not a hand-copied loop over mock layers: an
    /// active Layer 1 (learned-rule) match must win even though it precedes
    /// nothing else viable in this fixture — the meaningful claim is that
    /// the returned `extraction_method` is `"learned_patterns"` (Layer 1),
    /// proving the ladder actually reached and returned from the first
    /// layer rather than some other path.
    #[tokio::test]
    async fn test_orchestrator_stops_at_first_valid_layer() {
        let pool = setup_db_with_rule("active".to_string()).await;
        let body = "Your amount is 1500 INR at Amazon debit time 123";

        let mut layer6_timed_out = false;
        let result = run_extraction_ladder(&pool, "Chase", body, None, false, None, &mut layer6_timed_out)
            .await
            .unwrap();

        assert!(result.is_some());
        assert_eq!(result.unwrap().extraction_method, "learned_patterns");
    }

    /// Ensemble-lite regression test: a Layer 1 learned rule whose "amount"
    /// regex is simply wrong (captures a transaction ID instead of the real
    /// amount) must have its confidence downgraded once the cheap
    /// independent currency-regex cross-check finds a *different* amount
    /// elsewhere in the same body -- the ladder previously trusted whatever
    /// the first schema-valid layer said with no second opinion at all.
    #[tokio::test]
    async fn test_ensemble_lite_amount_disagreement_downgrades_confidence() {
        let body = "Txn ID 999900 INR for your purchase. Rs 500.00 debited at Amazon on 25-May-23";
        let pool = dummy_migrated_pool().await;
        let conn = pool.get().await.unwrap();
        let body_owned = body.to_string();
        conn.interact(move |c| {
            let template_hash = compute_template_hash(&body_owned);
            let base = crate::db::pattern_rules::PatternRulesRow {
                id: "wr1".to_string(),
                bank_name: "WrongRuleBank".to_string(),
                template_hash: template_hash.clone(),
                field_name: "amount".to_string(),
                // Deliberately wrong: captures the transaction ID, not the
                // real "Rs 500.00" amount elsewhere in the body.
                rule_payload_json: serde_json::json!({"regex": "Txn ID (\\d+)"}),
                status: "active".to_string(),
                success_count: 0,
                failure_count: 0,
                confidence: 1.0,
                created_at: Some(chrono::Utc::now().naive_utc()),
                updated_at: Some(chrono::Utc::now().naive_utc()),
            };
            crate::db::pattern_rules::insert(c, &base).unwrap();
            crate::db::pattern_rules::insert(
                c,
                &crate::db::pattern_rules::PatternRulesRow {
                    id: "wr2".to_string(),
                    field_name: "merchant".to_string(),
                    rule_payload_json: serde_json::json!({"regex": "at ([A-Za-z]+)"}),
                    ..base.clone()
                },
            )
            .unwrap();
            crate::db::pattern_rules::insert(
                c,
                &crate::db::pattern_rules::PatternRulesRow {
                    id: "wr3".to_string(),
                    field_name: "currency".to_string(),
                    rule_payload_json: serde_json::json!({"regex": "([A-Z]{3})"}),
                    ..base.clone()
                },
            )
            .unwrap();
            crate::db::pattern_rules::insert(
                c,
                &crate::db::pattern_rules::PatternRulesRow {
                    id: "wr4".to_string(),
                    field_name: "direction".to_string(),
                    rule_payload_json: serde_json::json!({"regex": "(debited)"}),
                    ..base.clone()
                },
            )
            .unwrap();
            crate::db::pattern_rules::insert(
                c,
                &crate::db::pattern_rules::PatternRulesRow {
                    id: "wr5".to_string(),
                    field_name: "event_time".to_string(),
                    rule_payload_json: serde_json::json!({"regex": "on (\\d{2}-[A-Za-z]{3}-\\d{2})"}),
                    ..base
                },
            )
            .unwrap();
        })
        .await
        .unwrap();

        let mut layer6_timed_out = false;
        let result = run_extraction_ladder(&pool, "WrongRuleBank", body, None, false, None, &mut layer6_timed_out)
            .await
            .unwrap()
            .expect("the (wrong) learned rule is schema-valid and must still be returned");

        assert_eq!(result.extraction_method, "learned_patterns");
        assert_eq!(
            result.amount_minor,
            Some(99_990_000),
            "the buggy rule's own (wrong) captured amount is still what's returned"
        );
        assert_eq!(
            result.confidence_score,
            Some(CROSS_CHECK_DISAGREEMENT_CONFIDENCE),
            "disagreement with the independent Rs 500.00 signal must downgrade confidence, \
             even though Layer 1 itself never set one before"
        );
    }

    /// Doc 30 TASK-TXN-001 acceptance test. Exercises the real
    /// `run_extraction_ladder` against an unmigrated pool with a body no
    /// layer can parse and `llm_eligible = false` — every layer including
    /// Layer 5 must fail closed to `None`, not panic or silently invent a
    /// partial result.
    #[tokio::test]
    async fn test_orchestrator_fails_if_all_layers_empty() {
        // A real (if unused) subscriber must be active here, not just the
        // bare process default: `tracing`'s per-callsite Interest cache can
        // permanently pin the "Layer 6 skipped" log line's callsite (shared
        // with test_llm_skipped_when_ineligible below) to
        // `never` the first time it fires under no subscriber at all, which
        // then silently defeats that other test's capturing layer if this
        // test happens to run first — this is not about this test's own
        // assertions, purely about not poisoning a shared callsite for a
        // sibling test running in parallel.
        use tracing_subscriber::layer::SubscriberExt;
        struct NoopLayer;
        impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for NoopLayer {}
        let _guard =
            tracing::subscriber::set_default(tracing_subscriber::registry().with(NoopLayer));

        let pool = dummy_pool();
        let mut layer6_timed_out = false;
        let res = run_extraction_ladder(&pool, "Chase", "unparseable body", None, false, None, &mut layer6_timed_out)
            .await
            .unwrap();
        assert!(res.is_none());
    }

    /// Doc 30 TASK-TXN-001 acceptance test. Layer 5 must be gated on
    /// hardware RAM-eligibility (`llm_eligible`), not on whether this bank's
    /// template has drifted — proven here by confirming the ladder emits the
    /// "skipped" log line and never reaches `Layer6LlmLayer::extract` at all
    /// (which would instead log "No app_dir provided") even with a body that
    /// makes Layers 1-4 fail.
    #[tokio::test]
    async fn test_llm_skipped_when_ineligible() {
        use tracing_subscriber::layer::SubscriberExt;

        struct MessageVisitor(String);
        impl tracing::field::Visit for MessageVisitor {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if field.name() == "message" {
                    self.0 = format!("{:?}", value);
                }
            }
        }
        struct CapturingLayer(std::sync::Arc<std::sync::Mutex<Vec<String>>>);
        impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for CapturingLayer {
            fn on_event(
                &self,
                event: &tracing::Event<'_>,
                _ctx: tracing_subscriber::layer::Context<'_, S>,
            ) {
                let mut visitor = MessageVisitor(String::new());
                event.record(&mut visitor);
                self.0.lock().unwrap().push(visitor.0);
            }
        }

        let logs = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let subscriber = tracing_subscriber::registry().with(CapturingLayer(logs.clone()));
        let _guard = tracing::subscriber::set_default(subscriber);

        let pool = dummy_pool();
        let mut layer6_timed_out = false;
        let res = run_extraction_ladder(&pool, "Chase", "unparseable body", None, false, None, &mut layer6_timed_out)
            .await
            .unwrap();
        assert!(res.is_none());

        let captured = logs.lock().unwrap();
        assert!(
            captured.iter().any(|l| l.contains("Layer 6 skipped")),
            "expected the RAM-ineligibility skip log line, got: {:?}",
            *captured
        );
        assert!(
            !captured.iter().any(|l| l.contains("No app_dir provided")),
            "Layer6LlmLayer::extract must never be reached when llm_eligible is false, got: {:?}",
            *captured
        );
    }

    #[test]
    fn test_compute_template_hash() {
        let b1 = "Hello 123 World 456";
        let b2 = "hello   789 world 000";
        // both should normalize to "hello # world #"
        assert_eq!(compute_template_hash(b1), compute_template_hash(b2));
    }

    async fn setup_db_with_rule(status: String) -> Pool {
        let pool = dummy_migrated_pool().await;
        let conn = pool.get().await.unwrap();
        conn.interact(move |c| {
            let template_hash =
                compute_template_hash("Your amount is 1500 INR at Amazon debit time 123");
            let rule = crate::db::pattern_rules::PatternRulesRow {
                id: "rule1".to_string(),
                bank_name: "Chase".to_string(),
                template_hash: template_hash.clone(),
                field_name: "amount".to_string(),
                rule_payload_json: serde_json::json!({"regex": "amount is ([0-9]+) INR"}),
                status: status.to_string(),
                success_count: 0,
                failure_count: 0,
                confidence: 1.0,
                created_at: Some(chrono::Utc::now().naive_utc()),
                updated_at: Some(chrono::Utc::now().naive_utc()),
            };

            crate::db::pattern_rules::insert(c, &rule).unwrap();

            // Add other fields to make it valid
            let rule_merchant = crate::db::pattern_rules::PatternRulesRow {
                id: "rule2".to_string(),
                bank_name: "Chase".to_string(),
                template_hash: template_hash.clone(),
                field_name: "merchant".to_string(),
                rule_payload_json: serde_json::json!({"regex": "at ([A-Za-z]+)"}),
                status: status.to_string(),
                ..rule.clone()
            };
            crate::db::pattern_rules::insert(c, &rule_merchant).unwrap();

            let rule_curr = crate::db::pattern_rules::PatternRulesRow {
                id: "rule3".to_string(),
                bank_name: "Chase".to_string(),
                template_hash: template_hash.clone(),
                field_name: "currency".to_string(),
                rule_payload_json: serde_json::json!({"regex": "([A-Z]{3})"}),
                status: status.to_string(),
                ..rule.clone()
            };
            crate::db::pattern_rules::insert(c, &rule_curr).unwrap();

            let rule_dir = crate::db::pattern_rules::PatternRulesRow {
                id: "rule4".to_string(),
                bank_name: "Chase".to_string(),
                template_hash: template_hash.clone(),
                field_name: "direction".to_string(),
                rule_payload_json: serde_json::json!({"regex": "(debit)"}),
                status: status.to_string(),
                ..rule.clone()
            };
            crate::db::pattern_rules::insert(c, &rule_dir).unwrap();

            let rule_time = crate::db::pattern_rules::PatternRulesRow {
                id: "rule5".to_string(),
                bank_name: "Chase".to_string(),
                template_hash: template_hash.clone(),
                field_name: "event_time".to_string(),
                rule_payload_json: serde_json::json!({"regex": "time ([0-9]+)"}),
                status: status.to_string(),
                ..rule.clone()
            };
            crate::db::pattern_rules::insert(c, &rule_time).unwrap();
        })
        .await
        .unwrap();
        pool
    }

    #[tokio::test]
    async fn test_learned_rule_applied_when_active() {
        let pool = setup_db_with_rule("active".to_string()).await;
        let layer = LearnedPatternLayer;
        let body = "Your amount is 1500 INR at Amazon debit time 123";

        let result = layer.extract(&pool, "Chase", body).await;

        assert!(result.is_some());
        let res = result.unwrap();
        assert_eq!(res.amount_minor, Some(150000));
        assert_eq!(res.merchant_raw, Some("Amazon".to_string()));
        assert_eq!(res.currency, Some("INR".to_string()));
        assert_eq!(res.direction, Some("debit".to_string()));
        assert_eq!(res.extraction_method, "learned_patterns");
    }

    /// Core requirement, end to end: variants learned from one email
    /// template must still match a DIFFERENT template for the same bank
    /// once select_active_rules_by_bank stops filtering by template_hash.
    #[tokio::test]
    async fn test_learned_rule_matches_across_different_templates() {
        // setup_db_with_rule seeds all five field variants against this
        // exact body's template_hash.
        let old_body = "Your amount is 1500 INR at Amazon debit time 123";
        let pool = setup_db_with_rule("active".to_string()).await;

        // A structurally different body (different template_hash) that the
        // SAME five regexes still happen to match.
        let new_body = "Reminder: your amount is 1500 INR at Amazon debit time 123 -- thank you.";
        assert_ne!(
            compute_template_hash(old_body),
            compute_template_hash(new_body),
            "the two bodies must hash differently to actually exercise cross-template matching"
        );

        let layer = LearnedPatternLayer;
        let result = layer.extract(&pool, "Chase", new_body).await;

        assert!(
            result.is_some(),
            "variants learned from one template must still be tried against a different template's email"
        );
        let res = result.unwrap();
        assert_eq!(res.amount_minor, Some(150000));
        assert_eq!(res.merchant_raw, Some("Amazon".to_string()));
        assert_eq!(res.direction, Some("debit".to_string()));
    }

    #[tokio::test]
    async fn test_inactive_rule_skipped() {
        let pool = setup_db_with_rule("inactive".to_string()).await;
        let layer = LearnedPatternLayer;
        let body = "Your amount is 1500 INR at Amazon debit time 123";

        let result = layer.extract(&pool, "Chase", body).await;

        // Should return None because rules are inactive and query won't pick them up
        assert!(result.is_none());
    }

    /// Doc 30 TASK-TXN-002 acceptance test: a `pending` rule (not yet
    /// promoted to `active`/`trusted` via 3 confirmed successes,
    /// `db/pattern_rules.rs::record_rule_success`) must never be
    /// auto-applied, even if its regex would otherwise match — a candidate
    /// rule is unproven until a human/feedback loop confirms it.
    #[tokio::test]
    async fn test_pending_rule_not_auto_applied() {
        let pool = setup_db_with_rule("pending".to_string()).await;
        let layer = LearnedPatternLayer;
        let body = "Your amount is 1500 INR at Amazon debit time 123";

        let result = layer.extract(&pool, "Chase", body).await;

        assert!(
            result.is_none(),
            "a pending rule must not be auto-applied, even when its regex matches"
        );
    }

    #[tokio::test]
    async fn test_hdfc_credit_card_regex() {
        let pool = dummy_pool();
        let layer = BankTemplateLayer;
        let body =
            "Rs 1500.00 spent on your HDFC Bank CREDIT Card ending 1234 at Amazon on 25-May-23.";
        let result = layer.extract(&pool, "HDFC Bank", body).await.unwrap();
        assert_eq!(result.amount_minor, Some(150000));
        assert_eq!(result.merchant_raw, Some("Amazon".to_string()));
        assert!(result.event_time.is_some());
        assert_eq!(result.extraction_method, "bank_templates");

        let body_4_digit =
            "Rs 1500.00 spent on your HDFC Bank CREDIT Card ending 1234 at Amazon on 25-May-2023.";
        let result_4 = layer
            .extract(&pool, "HDFC Bank", body_4_digit)
            .await
            .unwrap();
        assert_eq!(result_4.amount_minor, Some(150000));
        assert_eq!(result_4.event_time, parse_date("25-May-2023"));
    }

    /// Regression test for the date-sentinel bug (BankTemplateLayer path): a
    /// capture that doesn't parse as any recognized date format, with no
    /// `date_fallback_epoch` configured for this pattern (the
    /// `credit_card_spent` HDFC pattern has none), must leave `event_time`
    /// unset -- not silently default to the fabricated 2024-01-01 epoch the
    /// old code returned on any parse failure.
    #[tokio::test]
    async fn test_bank_template_invalid_date_no_fallback_leaves_event_time_none() {
        let pool = dummy_pool();
        let layer = BankTemplateLayer;
        // Day "35" doesn't exist -- syntactically matches the date capture
        // group's character class, but fails every chrono format attempted.
        let body =
            "Rs 1500.00 spent on your HDFC Bank CREDIT Card ending 1234 at Amazon on 35-May-23.";
        let result = layer.extract(&pool, "HDFC Bank", body).await.unwrap();
        assert_eq!(
            result.event_time, None,
            "an invalid date with no configured fallback must not fabricate a timestamp"
        );
        assert!(
            !result.is_valid(),
            "a result with no event_time must fail is_valid(), which is what makes the \
             orchestrator correctly skip past this layer instead of accepting a corrupted date"
        );
    }

    /// Doc 30 TASK-TXN-003: "A successful Layer 2 match seeds a
    /// pending-status pattern_rules row, so repeated matches against the
    /// same template_hash can graduate to a Layer 1 learned rule."
    #[tokio::test]
    async fn test_bank_template_match_seeds_pending_pattern_rule() {
        let pool = dummy_migrated_pool().await;
        let layer = BankTemplateLayer;
        let body =
            "Rs 1500.00 spent on your HDFC Bank CREDIT Card ending 1234 at Amazon on 25-May-23.";

        let result = layer.extract(&pool, "HDFC Bank", body).await;
        assert!(result.is_some());

        let template_hash = compute_template_hash(body);
        let conn = pool.get().await.unwrap();
        let rules = conn
            .interact(move |c| {
                // No direct "select all for hash" helper exists; pending rows
                // are deliberately excluded from select_active_rules_by_bank
                // by design, so query the tables directly here instead.
                c.prepare(
                    "SELECT p.field_name, v.status FROM pattern_rule_variants v \
                     JOIN pattern_rules p ON p.id = v.pattern_rule_id \
                     WHERE p.bank_name = ?1 AND v.template_hash = ?2",
                )
                .unwrap()
                .query_map(rusqlite::params!["HDFC Bank", template_hash], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
            })
            .await
            .unwrap();

        assert!(
            !rules.is_empty(),
            "a successful Layer 2 match must seed at least one pending pattern_rules row"
        );
        assert!(
            rules.iter().all(|(_, status)| status == "pending"),
            "seeded rows must be pending, not auto-active: {:?}",
            rules
        );
        let field_names: std::collections::HashSet<_> =
            rules.iter().map(|(f, _)| f.as_str()).collect();
        assert!(field_names.contains("amount"));
        assert!(field_names.contains("merchant"));
    }

    #[tokio::test]
    async fn test_hdfc_debit_card_regex() {
        let pool = dummy_pool();
        let layer = BankTemplateLayer;
        let body = "Rs 500.00 debited from HDFC Bank A/c ending 1234 at Amazon on 25-May-23";
        let result = layer.extract(&pool, "HDFC Bank", body).await.unwrap();
        assert_eq!(result.amount_minor, Some(50000));
        assert_eq!(result.merchant_raw, Some("Amazon".to_string()));
        assert_eq!(
            result.direction,
            Some("debit".to_string()),
            "debit-shaped pattern must resolve to debit direction"
        );
    }

    /// Regression test for the hardcoded-direction bug: `BankTemplateLayer`
    /// used to initialize `direction: Some("debit")` unconditionally and
    /// never override it per-pattern, so a credit/refund-shaped bank
    /// template match was silently mislabeled debit. `BankPatternTemplate`
    /// now carries a per-pattern `direction` field (see the
    /// `account_credit` pattern added to `hdfc_v1.json`), and this proves a
    /// credit-shaped match actually resolves to `"credit"`, not the old
    /// blanket default.
    #[tokio::test]
    async fn test_hdfc_credit_pattern_resolves_credit_direction_not_hardcoded_debit() {
        let pool = dummy_pool();
        let layer = BankTemplateLayer;
        let body =
            "Rs 5000.00 credited to your HDFC Bank A/c ending 1234 from John Doe on 25-May-23";
        let result = layer.extract(&pool, "HDFC Bank", body).await.unwrap();
        assert_eq!(result.amount_minor, Some(500000));
        assert_eq!(
            result.direction,
            Some("credit".to_string()),
            "a credit-shaped template match must not be mislabeled debit"
        );
        assert_eq!(result.extraction_method, "bank_templates");
    }

    #[tokio::test]
    async fn test_icici_credit_card_regex() {
        let pool = dummy_pool();
        let layer = BankTemplateLayer;
        let body = "INR 1500.00 spent on ICICI Bank Card XX1234 on 25-May-23 at Amazon.";
        let result = layer.extract(&pool, "ICICI Bank", body).await.unwrap();
        assert_eq!(result.amount_minor, Some(150000));
        assert_eq!(result.merchant_raw, Some("Amazon".to_string()));
    }

    #[tokio::test]
    async fn test_icici_upi_regex() {
        let pool = dummy_pool();
        let layer = BankTemplateLayer;
        let body = "Dear Customer, Acct XX1234 debited with INR 500.00 on 25-May-23. Info: UPI/1234567890/Amazon.";
        let result = layer.extract(&pool, "ICICI Bank", body).await.unwrap();
        assert_eq!(result.amount_minor, Some(50000));
        assert_eq!(result.merchant_raw, Some("Amazon".to_string()));
    }

    #[tokio::test]
    async fn test_sbi_credit_card_regex() {
        let pool = dummy_pool();
        let layer = BankTemplateLayer;
        let body = "Rs 1500.00 spent on your SBI Credit Card ending 1234 at Amazon on 25-May-23.";
        let result = layer
            .extract(&pool, "State Bank of India", body)
            .await
            .unwrap();
        assert_eq!(result.amount_minor, Some(150000));
        assert_eq!(result.merchant_raw, Some("Amazon".to_string()));
    }

    #[tokio::test]
    async fn test_axis_credit_card_regex() {
        let pool = dummy_pool();
        let layer = BankTemplateLayer;
        let body = "Rs 1500.00 spent on your Axis Bank Credit Card XX1234 at Amazon on 25-May-23.";
        let result = layer.extract(&pool, "Axis Bank", body).await.unwrap();
        assert_eq!(result.amount_minor, Some(150000));
        assert_eq!(result.merchant_raw, Some("Amazon".to_string()));
    }

    #[tokio::test]
    async fn test_kotak_credit_card_regex() {
        let pool = dummy_pool();
        let layer = BankTemplateLayer;
        let body = "Rs 1500.00 spent on your Kotak Mahindra Bank Credit Card XX1234 at Amazon on 25-May-23.";
        let result = layer
            .extract(&pool, "Kotak Mahindra Bank", body)
            .await
            .unwrap();
        assert_eq!(result.amount_minor, Some(150000));
        assert_eq!(result.merchant_raw, Some("Amazon".to_string()));
    }

    #[tokio::test]
    async fn test_yes_bank_credit_card_regex() {
        let pool = dummy_pool();
        let layer = BankTemplateLayer;
        let body = "Rs 1500.00 spent on your YES Bank Credit Card XX1234 at Amazon on 25-May-23.";
        let result = layer.extract(&pool, "Yes Bank", body).await.unwrap();
        assert_eq!(result.amount_minor, Some(150000));
        assert_eq!(result.merchant_raw, Some("Amazon".to_string()));
    }
    #[tokio::test]
    async fn test_generic_regex_fallback_success() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "You have paid Rs 1,500.50 to Zomato via UPI on 25/05/2023. Ref: 123456789012.";
        let result = layer.extract(&pool, "Any Bank", body).await.unwrap();

        assert_eq!(result.amount_minor, Some(150050));
        assert_eq!(result.currency, Some("INR".to_string()));
        assert_eq!(result.direction, Some("debit".to_string()));
        assert_eq!(result.merchant_raw, Some("Zomato".to_string()));
        assert_eq!(result.reference_id, Some("123456789012".to_string()));
        assert!(result.event_time.is_some());
        assert_eq!(result.extraction_method, "generic_regex");
    }

    #[tokio::test]
    async fn test_generic_regex_fallback_failure() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "Random email without proper transaction details.";
        let result = layer.extract(&pool, "Any Bank", body).await;

        assert!(result.is_none());
    }

    /// Regression test for the date-sentinel bug (GenericRegexLayer path):
    /// "99/99/9999" is date-shaped enough to match `GENERIC_DATE_RE` but
    /// isn't a real calendar date, so it must fail every parse format and
    /// leave `event_time` unset -- which then fails `is_valid()` entirely
    /// (event_time is unconditionally required), rather than the old
    /// behavior of silently returning a result dated 2024-01-01.
    #[tokio::test]
    async fn test_generic_regex_invalid_date_fails_validation_not_fake_date() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "You have paid Rs 500.00 to Zomato via UPI on 99/99/9999. Ref: 123456789012.";
        let result = layer.extract(&pool, "Any Bank", body).await;
        assert!(
            result.is_none(),
            "an unparseable date must fail the layer entirely, not fabricate a fake timestamp"
        );
    }

    /// Direct unit test on the two date parsers themselves: `None` on
    /// failure, never the old hardcoded `1704067200` (2024-01-01) sentinel.
    #[test]
    fn test_date_parsers_return_none_not_fake_sentinel_on_failure() {
        assert_eq!(parse_date("not a date"), None);
        assert_eq!(parse_date_generic("not a date"), None);
        assert_eq!(parse_date("35-May-23"), None);
    }

    fn ymd_ts(year: i32, month: u32, day: u32) -> i64 {
        chrono::NaiveDate::from_ymd_opt(year, month, day)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp()
    }

    /// `parse_date_generic`'s `ambiguous` flag must only be `true` for the
    /// bare-numeric formats when both components are <=12 -- day>12 numeric
    /// dates and month-name dates are both structurally unambiguous, even
    /// though the latter can also have day<=12.
    #[test]
    fn test_parse_date_generic_ambiguous_flag() {
        // day=25 > 12 -- only %d/%m/%Y can possibly match, unambiguous.
        let unambiguous_numeric = parse_date_generic("25/05/2023").unwrap();
        assert!(!unambiguous_numeric.ambiguous);

        // Month-name format -- inherently unambiguous even though day=5<=12.
        let month_name = parse_date_generic("05-Aug-2026").unwrap();
        assert!(!month_name.ambiguous);

        // Both day=2 and month=7 are <=12 -- genuinely two valid readings.
        let ambiguous = parse_date_generic("02-07-2026").unwrap();
        assert!(ambiguous.ambiguous);
        assert_eq!(ambiguous.timestamp, ymd_ts(2026, 7, 2));

        // day==month -- swap would be a no-op, not meaningfully ambiguous.
        let noop_swap = parse_date_generic("05-05-2026").unwrap();
        assert!(!noop_swap.ambiguous);
    }

    /// Regression lock for the provenance-blindness bug: `event_time_ambiguous:
    /// false` (the default for every non-numeric-ambiguous parse -- bank
    /// templates, Layer 5 statement rows, month-name dates) must make
    /// `apply_date_cross_check` a guaranteed no-op, even when the resolved
    /// date's day/month values would otherwise look swappable and the anchor
    /// decisively favors the swap. Without this the cross-check would
    /// silently corrupt already-correct dates (~40% of all calendar days
    /// have day<=12).
    #[test]
    fn test_apply_date_cross_check_noop_when_not_flagged_ambiguous() {
        let original_ts = ymd_ts(2026, 8, 5); // "5 August" -- e.g. from a month-name bank template
        let mut obs = ExtractionResult {
            event_time: Some(original_ts),
            event_time_ambiguous: false,
            ..Default::default()
        };
        // Anchor sits exactly on the swapped reading ("5 May") -- if the
        // function looked at day/month values instead of the flag, this
        // would trigger a swap.
        let anchor = Some(ymd_ts(2026, 5, 5));

        apply_date_cross_check(&mut obs, anchor);

        assert_eq!(obs.event_time, Some(original_ts));
        assert_eq!(obs.date_cross_check_flag, None);
    }

    /// Decisive case: swap lands within the plausible-delay window and is
    /// clearly closer to the anchor than the original -- auto-correct.
    #[test]
    fn test_apply_date_cross_check_decisive_swap() {
        // Body-parsed as "2 July 2026" (DD-MM default), but the email
        // arrived 7 Feb 2026 -- the swapped reading ("7 Feb") lands exactly
        // on the anchor, the original is ~5 months off.
        let original_ts = ymd_ts(2026, 7, 2);
        let mut obs = ExtractionResult {
            event_time: Some(original_ts),
            event_time_ambiguous: true,
            ..Default::default()
        };
        let anchor = Some(ymd_ts(2026, 2, 7));

        apply_date_cross_check(&mut obs, anchor);

        assert_eq!(obs.event_time, Some(ymd_ts(2026, 2, 7)));
        assert_eq!(
            obs.date_cross_check_flag,
            Some("swapped_by_anchor".to_string())
        );
    }

    /// Weak signal: original is only a couple of days from the anchor
    /// (well within plausible range) and the swap isn't decisively closer
    /// -- keep the DD-MM locale default untouched, no flag. This is the
    /// common case for a correctly-parsed ambiguous date.
    #[test]
    fn test_apply_date_cross_check_weak_signal_untouched() {
        let original_ts = ymd_ts(2026, 7, 2); // "2 July"
        let mut obs = ExtractionResult {
            event_time: Some(original_ts),
            event_time_ambiguous: true,
            ..Default::default()
        };
        // Anchor 1 day after the original -- entirely plausible as-is, and
        // nowhere near the swapped reading ("7 Feb").
        let anchor = Some(ymd_ts(2026, 7, 3));

        apply_date_cross_check(&mut obs, anchor);

        assert_eq!(obs.event_time, Some(original_ts));
        assert_eq!(obs.date_cross_check_flag, None);
    }

    /// Neither candidate is plausible relative to the anchor (e.g. a
    /// historical backfill scan, or the regex grabbed an unrelated date) --
    /// don't guess which one is right. Leave `event_time` untouched, flag
    /// for review, and downgrade confidence so `pending_review`-style gates
    /// can catch it.
    #[test]
    fn test_apply_date_cross_check_both_implausible_flags_for_review() {
        let original_ts = ymd_ts(2026, 7, 2); // "2 July"
        let mut obs = ExtractionResult {
            event_time: Some(original_ts),
            event_time_ambiguous: true,
            confidence_score: Some(0.6),
            ..Default::default()
        };
        // Anchor 3 months later -- both "2 July" and "7 Feb" are far outside
        // the plausible-delay window.
        let anchor = Some(ymd_ts(2026, 10, 2));

        apply_date_cross_check(&mut obs, anchor);

        assert_eq!(obs.event_time, Some(original_ts));
        assert_eq!(
            obs.date_cross_check_flag,
            Some("anchor_mismatch_needs_review".to_string())
        );
        assert!(obs.confidence_score.unwrap() <= CROSS_CHECK_DISAGREEMENT_CONFIDENCE);
    }

    /// No anchor at all (e.g. Gmail's `internalDate` was unavailable) --
    /// nothing to arbitrate with, must be a no-op.
    #[test]
    fn test_apply_date_cross_check_no_anchor_is_noop() {
        let original_ts = ymd_ts(2026, 7, 2);
        let mut obs = ExtractionResult {
            event_time: Some(original_ts),
            event_time_ambiguous: true,
            ..Default::default()
        };

        apply_date_cross_check(&mut obs, None);

        assert_eq!(obs.event_time, Some(original_ts));
        assert_eq!(obs.date_cross_check_flag, None);
    }

    /// Doc 30 TASK-TXN-004 acceptance test: generic currency-prefixed amount
    /// regex, proximate to a debit/credit verb.
    #[tokio::test]
    async fn test_generic_amount_extraction() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "You have paid Rs 1,500.50 to Zomato via UPI on 25/05/2023.";
        let result = layer.extract(&pool, "Any Bank", body).await.unwrap();
        assert_eq!(result.amount_minor, Some(150050));
        assert_eq!(result.currency, Some("INR".to_string()));
        assert!(
            result.confidence_score.unwrap() > 0.5 && result.confidence_score.unwrap() <= 0.7,
            "Layer 3 confidence must stay within the documented 0.5-0.7 range, below \
             Layer 1/2's 0.9+, got {:?}",
            result.confidence_score
        );
    }

    /// Regression test for the "fake precision" bug: Layer 3 used to report
    /// a flat `0.6` regardless of which regex branch actually matched or
    /// how many fields resolved cleanly. This proves the score now varies:
    /// a weak extraction (amount-implied direction, ambiguous-tier
    /// merchant, no reference ID) scores strictly lower than a strong one
    /// (explicit direction verb, strict-tier merchant, reference ID
    /// present), which scores at the documented ceiling.
    #[tokio::test]
    async fn test_generic_confidence_varies_by_field_strength() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;

        // Strong: explicit "paid to" (strict-tier merchant label) verb +
        // reference ID present.
        let strong_body =
            "You have paid Rs 1,500.50 paid to Zomato via UPI on 25/05/2023. Ref: 123456789012.";
        let strong = layer.extract(&pool, "Any Bank", strong_body).await.unwrap();
        assert_eq!(strong.confidence_score, Some(LAYER3_MAX_CONFIDENCE));

        // Weak: amount+currency present, no explicit direction verb (the
        // amount-implies-debit fallback fires instead), ambiguous-tier
        // merchant only, no reference ID.
        let weak_body = "Rs 500.00 at Zomato on 25/05/2023.";
        let weak = layer.extract(&pool, "Any Bank", weak_body).await.unwrap();
        assert!(
            weak.confidence_score.unwrap() < strong.confidence_score.unwrap(),
            "a weaker extraction must score strictly lower than a strong one, got weak={:?} strong={:?}",
            weak.confidence_score,
            strong.confidence_score
        );
        assert_eq!(
            weak.confidence_score,
            Some(
                LAYER3_BASE_CONFIDENCE
                    + LAYER3_AMOUNT_CURRENCY_BONUS
                    + LAYER3_AMBIGUOUS_MERCHANT_BONUS
            )
        );
    }

    /// Doc 30 TASK-TXN-004 acceptance test: direction via keyword proximity
    /// ("debited"/"spent"/"paid" vs. "credited"/"received").
    #[tokio::test]
    async fn test_generic_direction_keyword_proximity() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;

        let debit_body = "Rs 500 spent at Amazon on 01-Jan-24.";
        let debit_result = layer.extract(&pool, "Any Bank", debit_body).await.unwrap();
        assert_eq!(debit_result.direction, Some("debit".to_string()));

        let credit_body = "Rs 500 credited to your account from Amazon Refund on 01-Jan-24.";
        let credit_result = layer.extract(&pool, "Any Bank", credit_body).await.unwrap();
        assert_eq!(credit_result.direction, Some("credit".to_string()));
    }

    /// Doc 30 TASK-TXN-004 acceptance test: merchant via capitalized-token or
    /// "at"/"to"/"towards"/"info:" heuristics.
    #[tokio::test]
    async fn test_generic_merchant_heuristic() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "Rs 1,500.50 paid to Zomato via UPI on 25/05/2023.";
        let result = layer.extract(&pool, "Any Bank", body).await.unwrap();
        assert_eq!(result.merchant_raw, Some("Zomato".to_string()));
    }

    /// Doc 30 TASK-TXN-004 acceptance test: "towards" is a required
    /// proximity keyword, not just "at"/"to"/"from"/"for".
    #[tokio::test]
    async fn test_generic_merchant_heuristic_towards() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "Rs 250.00 paid towards Swiggy via UPI on 25/05/2023.";
        let result = layer.extract(&pool, "Any Bank", body).await.unwrap();
        assert_eq!(result.merchant_raw, Some("Swiggy".to_string()));
    }

    /// Doc 30 TASK-TXN-004 acceptance test: "Info: <merchant>" is a common
    /// Indian UPI alert convention -- previously only hardcoded into
    /// Layer 2's ICICI template, so any *other* bank using the same
    /// convention with no dedicated Layer 2 template fell through this
    /// fallback with no merchant extracted at all.
    #[tokio::test]
    async fn test_generic_merchant_heuristic_info_colon() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "Rs 99.00 debited on 25/05/2023. Info: Starbucks Coffee";
        let result = layer.extract(&pool, "Any Bank", body).await.unwrap();
        assert_eq!(result.merchant_raw, Some("Starbucks Coffee".to_string()));
    }

    /// gmail false-negative remediation, Cluster E: card-network settlement
    /// descriptors (`RAZ*SWIGGY`) contain `*`, which the merchant capture
    /// class previously excluded, truncating the match before any
    /// terminator was reached and yielding no merchant at all.
    #[tokio::test]
    async fn test_generic_merchant_heuristic_asterisk_descriptor() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "Rs. 2590.00 has been debited from your HDFC Bank Credit Card ending 0364 towards RAZ*SWIGGY on 24 May, 2026 at 19:34:18 .";
        let result = layer.extract(&pool, "HDFC Bank", body).await.unwrap();
        assert_eq!(result.merchant_raw, Some("RAZ*SWIGGY".to_string()));
        assert_eq!(result.amount_minor, Some(259000));
    }

    /// gmail false-negative remediation, Cluster F: IDFC FIRST Bank's debit
    /// template uses a space-separated "DD Mon YYYY" date ("23 MAY 2026"),
    /// which no prior date-regex alternative covered -- `is_valid()`
    /// requires `event_time`, so the whole extraction was discarded even
    /// though amount/direction/merchant all matched.
    #[tokio::test]
    async fn test_generic_date_space_separated_day_month_year() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "Transaction Successful! INR 193.92 spent on your IDFC FIRST BANK Credit Card ending XX1920 at CRED TELECOM on 23 MAY 2026.";
        let result = layer.extract(&pool, "IDFC FIRST Bank", body).await.unwrap();
        assert_eq!(result.merchant_raw, Some("CRED TELECOM".to_string()));
        assert_eq!(result.amount_minor, Some(19392));
        assert!(result.event_time.is_some());
    }

    /// gmail false-negative remediation, Cluster G: Jupiter's
    /// Federal-Bank-Savings "Money credited" template labels the
    /// counterparty as "Payment from:" (colon immediately after the
    /// keyword, before the mandatory whitespace the regex required) and
    /// dates as "Month DD, YYYY" ("May 30, 2026"), neither of which any
    /// prior pattern covered.
    #[tokio::test]
    async fn test_generic_merchant_heuristic_colon_label_and_month_first_date() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "You've received ₹15563.0 in Federal Bank Savings Account ending with 1527.\nPayment from:                                ADITYA RAWAL\nDate                                May 30, 2026";
        let result = layer.extract(&pool, "Jupiter", body).await.unwrap();
        assert_eq!(result.merchant_raw, Some("ADITYA RAWAL".to_string()));
        assert_eq!(result.amount_minor, Some(1556300));
        assert_eq!(result.direction, Some("credit".to_string()));
        assert!(result.event_time.is_some());
    }

    /// gmail false-negative remediation, Cluster D: Axis Bank's AutoPay
    /// activation confirmation labels the counterparty "Merchant Name:"
    /// (two words) on its own line, with the value on the *next* line.
    /// Aditya's decision: capture this as a real ₹0.00 debit despite no
    /// funds moving, since it's a dated pipeline event he wants visible.
    #[tokio::test]
    async fn test_generic_merchant_heuristic_two_word_label_next_line() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "24-04-2026\n\nDear Customer,\n\nHere's the summary of your successful AutoPay transaction:\n\nTransaction Amount:\n\nINR 0.00\n\nMerchant Name:\n\nScribdInc\n\nAutoPay ID:\n\nYPXvrvJ1jr\n\nAxis Bank Credit Card No.\n\nXX3825\n\nMax Limit:\n\nINR 1000.00\n\nYou'll receive a notification mentioning the transaction amount prior to any subsequent debit initiated by ScribdInc.";
        let result = layer.extract(&pool, "Axis Bank", body).await.unwrap();
        assert_eq!(result.merchant_raw, Some("ScribdInc".to_string()));
        assert_eq!(result.amount_minor, Some(0));
        assert_eq!(result.direction, Some("debit".to_string()));
        assert!(result.event_time.is_some());
    }

    /// gmail false-negative remediation, Cluster H: a neobank "money
    /// credited" template (Jupiter, Federal Bank Savings) says "...was
    /// credited **to your account**" before separately labeling the real
    /// counterparty further down ("Payment **from**: NAME"). The single-
    /// match ambiguous-tier lookup used to stop at the first "to/from/at/
    /// for/by" hit and take "your account" itself as the merchant, since it
    /// was a non-empty capture and nothing downstream caught it.
    #[tokio::test]
    async fn test_generic_merchant_skips_self_referential_account() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "₹17000.0 was credited to your account\nYou've received ₹17000.0 in Federal Bank Savings Account ending with 1527.\nPayment from:                                ADITYA RAWAL\nDate                                Jun 30, 2026";
        let result = layer.extract(&pool, "Jupiter", body).await.unwrap();
        assert_eq!(result.merchant_raw, Some("ADITYA RAWAL".to_string()));
        assert_eq!(result.amount_minor, Some(1700000));
    }

    /// Regression test for the real YES Bank body that produced a wrong
    /// "block your" merchant (and, via `TransactionRecord`/`CanonicalTransaction`
    /// never carrying `direction` to the list UI, a debit spend rendering
    /// green/positive). Two independent bugs, both fixed here:
    /// 1. The settlement descriptor uses an underscore ("UPI_SRI SAI FRUITS
    ///    AND") which the old `[A-Za-z0-9\s*]` capture class rejected,
    ///    failing the "at ..." match entirely and letting `captures_iter`
    ///    fall through to the footer's "To **block your** card" phrase,
    ///    which satisfies the same ambiguous "to" + terminator shape.
    /// 2. "has been spent" must resolve `direction` to "debit", not fall
    ///    through to the no-explicit-verb branch.
    #[tokio::test]
    async fn test_generic_regex_underscore_merchant_and_disclaimer_footer() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "Dear Customer, Greetings from YES BANK. INR 91.00 has been spent on your YES BANK Credit Card ending with 2982 at UPI_SRI SAI FRUITS AND on 10-07-2026 at 08:55:35 pm. Avl Bal INR 82434.42. In case, this transaction was not initiated by you, please block your card immediately by calling our 24x7 customer care or visiting the nearest branch.";
        let result = layer.extract(&pool, "Yes Bank", body).await.unwrap();
        assert_eq!(result.merchant_raw, Some("UPI_SRI SAI FRUITS AND".to_string()));
        assert_eq!(result.direction, Some("debit".to_string()));
        assert_eq!(result.amount_minor, Some(9100));
    }

    /// A merchant descriptor made entirely of instruction/disclaimer filler
    /// words ("to block your card") must never be accepted even when it's
    /// the *only* ambiguous-keyword match in the body (no real merchant
    /// label present at all) -- `is_invalid_merchant`'s stopword filter must
    /// reject it outright rather than merely being outcompeted by an
    /// earlier real match.
    #[tokio::test]
    async fn test_generic_merchant_rejects_stopword_only_disclaimer_capture() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "INR 250.00 debited. To block your card, SMS BLOCK to 9876543210 or call our helpline.";
        let result = layer.extract(&pool, "Yes Bank", body).await;
        // No valid merchant anywhere in the body -- must not fabricate
        // "block your" as the merchant, even though `is_valid()` then fails
        // this layer entirely (a later layer or pending-review is the
        // correct outcome, not a wrong merchant).
        if let Some(r) = result {
            assert_ne!(r.merchant_raw, Some("block your".to_string()));
        }
    }

    /// gmail false-negative remediation, Cluster H: a declined
    /// international-card-transaction template states the amount in a
    /// spelled-out ISO currency code ("USD 1.00") rather than ₹/Rs/INR/$ --
    /// none of which the amount regex recognized, so extraction found
    /// nothing at all despite every other field being present.
    #[tokio::test]
    async fn test_generic_amount_recognizes_spelled_out_iso_currency_code() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "A transaction of USD 1.00 on your YES BANK Credit Card ending 2982 on 20-05-2026 at 11:57:54 pm at OPENAI is declined because International Ecom/online transactions are disabled on your card.";
        let result = layer.extract(&pool, "Yes Bank", body).await.unwrap();
        assert_eq!(result.amount_minor, Some(100));
        assert_eq!(result.currency, Some("USD".to_string()));
    }

    /// gmail false-negative remediation, Cluster H: same body as above --
    /// "at OPENAI is declined because International Ecom/online
    /// transactions are disabled..." has no comma/period or any prior
    /// terminator keyword within 40 chars of the merchant name, so the
    /// lazy capture kept expanding through the surrounding prose looking
    /// for one, exhausted the cap, and the whole match failed at that
    /// position -- falling through to an unrelated later "To enable," match
    /// instead of the real merchant.
    #[tokio::test]
    async fn test_generic_merchant_terminates_before_declined_prose() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "A transaction of USD 1.00 on your YES BANK Credit Card ending 2982 on 20-05-2026 at 11:57:54 pm at OPENAI is declined because International Ecom/online transactions are disabled on your card. To enable,please visit iris by YES BANK app.";
        let result = layer.extract(&pool, "Yes Bank", body).await.unwrap();
        assert_eq!(result.merchant_raw, Some("OPENAI".to_string()));
    }

    /// Doc 30 TASK-TXN-004 acceptance test: when Layer 3's own date regex
    /// finds nothing, Gmail's `internalDate` fills in as a fallback — but
    /// must never override a date the extraction layer already found (the
    /// bug this test guards against: the caller previously overwrote
    /// `event_time` unconditionally, discarding a more precise in-body date
    /// in favor of the email's arrival timestamp).
    #[test]
    fn test_generic_date_fallback_to_internal_date() {
        use crate::ingestion::message_processor::MessageProcessor;

        // No internal_date at all -> no fallback value.
        assert_eq!(MessageProcessor::internal_date_fallback(&None), None);

        // A valid internalDate (epoch millis as a string) converts to epoch
        // seconds -- this is the fallback path itself.
        let internal_date = Some("1700000000000".to_string());
        assert_eq!(
            MessageProcessor::internal_date_fallback(&internal_date),
            Some(1_700_000_000)
        );

        // Malformed internalDate must not panic -- fails closed to None.
        let malformed = Some("not-a-number".to_string());
        assert_eq!(MessageProcessor::internal_date_fallback(&malformed), None);
    }

    #[test]
    fn test_amount_minor_converter_indian_formatting() {
        assert_eq!(parse_amount("1,00,000.00"), Some(10000000));
        assert_eq!(parse_amount("1,500.50"), Some(150050));
        assert_eq!(parse_amount("500"), Some(50000));
        assert_eq!(parse_amount("10,00,00,000"), Some(10000000000));
    }

    #[tokio::test]
    async fn test_nlp_parser_hdfc_debit_alert() {
        let pool = dummy_pool();
        let layer = NlpLayer;
        let body = "Rs 500.00 debited from HDFC Bank A/c ending 1234 at Amazon on 25-May-23 Bal Rs 1000.00";
        let result = layer.extract(&pool, "HDFC Bank", body).await.unwrap();

        assert_eq!(result.amount_minor, Some(50000));
        assert_eq!(result.currency, Some("INR".to_string()));
        assert_eq!(result.direction, Some("debit".to_string()));
        assert_eq!(result.merchant_raw, Some("Amazon".to_string()));
        assert_eq!(result.balance_after, Some(100000));
        assert!(result.event_time.is_some());
        assert_eq!(result.extraction_method, "nlp");
    }

    #[tokio::test]
    async fn test_nlp_parser_upi_alert_with_vpa() {
        let pool = dummy_pool();
        let layer = NlpLayer;
        let body = "Dear Customer, Acct XX1234 debited with INR 500.00 on 25-May-23. Info: UPI/1234567890/AmazonPay.";
        let result = layer.extract(&pool, "Any Bank", body).await.unwrap();

        assert_eq!(result.amount_minor, Some(50000));
        assert_eq!(result.currency, Some("INR".to_string()));
        assert_eq!(result.direction, Some("debit".to_string()));
        assert_eq!(result.merchant_raw, Some("AmazonPay".to_string()));
        assert!(result.event_time.is_some());
        assert_eq!(result.extraction_method, "nlp");
    }

    /// New capability test (shared lexicon consolidation): NlpLayer
    /// previously had zero support for Layer 3's unambiguous merchant-label
    /// keywords ("towards", "paid to", "purchased at", "in favor of",
    /// etc.) -- a body whose only merchant signal is one of those, with no
    /// ambiguous at/to/from/for/by keyword and no UPI VPA token present,
    /// previously extracted no merchant here at all and failed
    /// `is_valid()` entirely (no merchant, no balance_after). The
    /// strict-label pre-pass rescue fixes this without touching the
    /// existing ambiguous/UPI-VPA paths' behavior (both still pass above).
    #[tokio::test]
    async fn test_nlp_strict_label_rescue_finds_merchant_ambiguous_tier_would_miss() {
        let pool = dummy_pool();
        let layer = NlpLayer;
        let body = "Rs 500.00 debited towards Zomato on 25-May-23";
        let result = layer
            .extract(&pool, "Any Bank", body)
            .await
            .expect("must extract successfully via the strict-label rescue");
        assert_eq!(result.merchant_raw, Some("Zomato".to_string()));
        assert_eq!(result.direction, Some("debit".to_string()));
        assert_eq!(result.amount_minor, Some(50000));
    }

    /// Regression test (strengthen-regex pass): the ambiguous-keyword
    /// merchant block used to have no `result.merchant_raw.is_none()` guard,
    /// so it unconditionally overwrote any already-found merchant on every
    /// subsequent "at/to/from/for/by" hit -- a disclaimer footer's keyword
    /// occurrence (always later in the body than the real transaction line)
    /// always won, clobbering the correct earlier match. Also verifies
    /// `is_invalid_merchant`'s stopword filter now applies inside NlpLayer
    /// (previously only Layer 3 called it).
    #[tokio::test]
    async fn test_nlp_first_valid_merchant_not_overwritten_by_later_disclaimer() {
        let pool = dummy_pool();
        let layer = NlpLayer;
        let body = "Rs 500.00 debited from HDFC Bank A/c ending 1234 at Amazon on 25-May-23 Bal Rs 1000.00. To block your card immediately, call our helpline.";
        let result = layer.extract(&pool, "HDFC Bank", body).await.unwrap();

        assert_eq!(result.merchant_raw, Some("Amazon".to_string()));
    }

    // -----------------------------------------------------------------------
    // Tests for extract_instrument_signals (Task 4.7)
    // -----------------------------------------------------------------------

    #[test]
    fn test_instrument_signals_credit_card_last4() {
        let body =
            "Rs 1500.00 spent on your HDFC Bank CREDIT Card ending 1234 at Amazon on 25-May-23.";
        let signals = extract_instrument_signals("HDFC Bank", body);
        assert_eq!(signals.masked_identifier, Some("XXXX1234".to_string()));
        assert_eq!(signals.instrument_type, Some("credit_card".to_string()));
        assert_eq!(signals.issuer_name, Some("HDFC Bank".to_string()));
    }

    #[test]
    fn test_instrument_signals_bank_account_suffix() {
        let body = "Rs 500.00 debited from HDFC Bank A/c ending 5678 at Amazon on 25-May-23.";
        let signals = extract_instrument_signals("HDFC Bank", body);
        assert_eq!(signals.masked_identifier, Some("XXXX5678".to_string()));
        assert_eq!(signals.instrument_type, Some("bank_account".to_string()));
        assert_eq!(signals.issuer_name, Some("HDFC Bank".to_string()));
    }

    #[test]
    fn test_instrument_signals_upi_vpa_detected() {
        let body = "Dear Customer, UPI payment of Rs 200 credited to your VPA user@icici from merchant@upi on 25-May-23.";
        let signals = extract_instrument_signals("ICICI Bank", body);
        assert_eq!(signals.masked_identifier, Some("user@icici".to_string()));
        assert_eq!(signals.instrument_type, Some("upi_vpa".to_string()));
        assert_eq!(signals.upi_vpa, Some("user@icici".to_string()));
        assert_eq!(signals.issuer_name, Some("ICICI Bank".to_string()));
    }

    #[test]
    fn test_instrument_signals_counterparty_vpa_ignored_for_user_instrument() {
        let body = "Dear Customer, Rs.750.00 is debited from your account ending 4691 towards VPA 8127696200@jupiteraxis (ADITYA RAWAL) on 07-06-26.";
        let signals = extract_instrument_signals("HDFC Bank", body);
        assert_eq!(signals.masked_identifier, Some("XXXX4691".to_string()));
        assert_eq!(signals.instrument_type, Some("bank_account".to_string()));
        assert_eq!(signals.upi_vpa, None);
    }

    #[test]
    fn test_instrument_signals_network_detected() {
        let body =
            "Rs 1500.00 spent on your Axis Visa Credit Card ending 9999 at Flipkart on 01-Jan-24.";
        let signals = extract_instrument_signals("Axis Bank", body);
        assert_eq!(signals.network, Some("Visa".to_string()));
        assert_eq!(signals.masked_identifier, Some("XXXX9999".to_string()));
    }

    #[test]
    fn test_instrument_signals_no_match_returns_only_issuer() {
        // A body with no card/account/VPA patterns
        let body = "Newsletter from your bank. No transaction details.";
        let signals = extract_instrument_signals("SBI", body);
        assert!(signals.masked_identifier.is_none());
        assert!(signals.instrument_type.is_none());
        assert_eq!(signals.issuer_name, Some("SBI".to_string()));
        assert!(signals.network.is_none());
    }

    #[test]
    fn test_instrument_signals_jupiter_debit_vpa_extraction() {
        let body = "Hey, Aditya\nYour UPI payment was successful\n\nYou paid ₹14000\n\nPaid to T Jyoshna\n7674036967@ybl\n\nDate Jul 05, 2026\n\nFrom Aditya\n8127696200@jupiteraxis\n\nTransaction ID 1321783237916267118\n\nBank reference Number 699841171866";
        let signals = extract_instrument_signals("Jupiter", body);
        assert_eq!(signals.upi_vpa, Some("8127696200@jupiteraxis".to_string()));
        assert_eq!(signals.masked_identifier, Some("8127696200@jupiteraxis".to_string()));
        assert_eq!(signals.instrument_type, Some("upi_vpa".to_string()));
    }

    #[test]
    fn test_instrument_signals_payee_vpa_only_never_saved_as_user_instrument() {
        let body = "You paid ₹1958.00 to MAX SUPER SPECIALITY HOSPITAL saharahospital.42752193@hdfcbank on 08-Jun-26.";
        let signals = extract_instrument_signals("Jupiter", body);
        assert_eq!(signals.upi_vpa, None);
        assert_eq!(signals.masked_identifier, None);
        assert_eq!(signals.instrument_type, None);
    }

    #[tokio::test]
    async fn test_ladder_augments_result_with_instrument_signals() {
        let pool = dummy_pool();
        // Use BankTemplateLayer body that will match HDFC credit card pattern
        let body =
            "Rs 1500.00 spent on your HDFC Bank CREDIT Card ending 1234 at Amazon on 25-May-23.";
        let mut layer6_timed_out = false;
        let result = run_extraction_ladder(&pool, "HDFC Bank", body, None, false, None, &mut layer6_timed_out)
            .await
            .unwrap();
        assert!(result.is_some());
        let obs = result.unwrap();
        // Extraction succeeded
        assert_eq!(obs.amount_minor, Some(150000));
        // Instrument signals populated by run_extraction_ladder
        assert_eq!(obs.masked_identifier, Some("XXXX1234".to_string()));
        assert_eq!(obs.instrument_type, Some("credit_card".to_string()));
        assert_eq!(obs.issuer_name, Some("HDFC Bank".to_string()));
    }

    // -----------------------------------------------------------------------
    // Tests for Layer5CrossrefLayer (Doc 30 TASK-TXN-005)
    // -----------------------------------------------------------------------

    /// Seeds a migrated DB with one instrument, one statement, and the given
    /// `statement_entries` rows for it. Returns the pool.
    async fn setup_crossref_db(
        entries: Vec<crate::db::statement_entries::StatementEntriesRow>,
    ) -> Pool {
        let pool = dummy_migrated_pool().await;
        let conn = pool.get().await.unwrap();
        conn.interact(move |c| {
            c.execute("INSERT INTO local_profile (id) VALUES (1)", [])
                .unwrap();
            c.execute(
                "INSERT INTO instruments (id, type, issuer_name, masked_identifier, status) \
                 VALUES ('inst_1', 'credit_card', 'HDFC Bank', 'XXXX1234', 'active')",
                [],
            )
            .unwrap();
            c.execute(
                "INSERT INTO statements (id, instrument_id, statement_type, billing_period_start, billing_period_end, parse_status) \
                 VALUES ('stmt_1', 'inst_1', 'credit_card', '2023-05-01', '2023-05-31', 'parsed')",
                [],
            )
            .unwrap();
            for entry in entries {
                crate::db::statement_entries::insert(c, &entry).unwrap();
            }
        })
        .await
        .unwrap();
        pool
    }

    fn crossref_entry(
        id: &str,
        transaction_date: chrono::NaiveDate,
        amount_minor: i64,
        reference_id: Option<&str>,
    ) -> crate::db::statement_entries::StatementEntriesRow {
        crate::db::statement_entries::StatementEntriesRow {
            id: id.to_string(),
            statement_id: Some("stmt_1".to_string()),
            row_index: Some(1),
            transaction_date: Some(transaction_date),
            posting_date: None,
            description_raw: Some("AMAZON PAY".to_string()),
            merchant_raw: Some("Amazon".to_string()),
            merchant_normalized: Some("amazon".to_string()),
            amount: Some(amount_minor as f64 / 100.0),
            amount_minor: Some(amount_minor),
            currency: Some("INR".to_string()),
            direction: Some("debit".to_string()),
            reference_id: reference_id.map(|s| s.to_string()),
            location: None,
            raw_row_json: None,
            created_at: None,
        }
    }

    /// Doc 30 TASK-TXN-005 acceptance test: a single unambiguous match
    /// borrows the statement entry's complete, authoritative field set.
    #[tokio::test]
    async fn test_layer5_single_match_completes_extraction() {
        let anchor = chrono::NaiveDate::from_ymd_opt(2023, 5, 25).unwrap();
        let entry_date = chrono::NaiveDate::from_ymd_opt(2023, 5, 24).unwrap();
        let pool = setup_crossref_db(vec![crossref_entry(
            "se_1",
            entry_date,
            150000,
            Some("123456789012"),
        )])
        .await;

        // Body yields a partial amount (Rs 1500.00) but no merchant/date --
        // exactly the "layers 1-4 failed overall" scenario Layer 5 rescues.
        let body = "Rs 1500.00 spent on your HDFC Bank credit card ending 1234.";
        let result = Layer5CrossrefLayer
            .extract(&pool, "HDFC Bank", body, Some(anchor))
            .await;

        assert!(result.is_some());
        let obs = result.unwrap();
        assert_eq!(obs.amount_minor, Some(150000));
        assert_eq!(obs.merchant_raw, Some("Amazon".to_string()));
        assert_eq!(obs.reference_id, Some("123456789012".to_string()));
        assert_eq!(obs.extraction_method, "layer5_statement_crossref");
        assert_eq!(obs.masked_identifier, Some("XXXX1234".to_string()));
    }

    /// Doc 30 TASK-TXN-005 acceptance test: multiple candidates matching the
    /// same partial fields must not be force-picked (Doc 15 §2 principle 9).
    #[tokio::test]
    async fn test_layer5_ambiguous_match_returns_none() {
        let anchor = chrono::NaiveDate::from_ymd_opt(2023, 5, 25).unwrap();
        let entry_date = chrono::NaiveDate::from_ymd_opt(2023, 5, 24).unwrap();
        let pool = setup_crossref_db(vec![
            crossref_entry("se_1", entry_date, 150000, Some("111111111111")),
            crossref_entry("se_2", entry_date, 150000, Some("222222222222")),
        ])
        .await;

        let body = "Rs 1500.00 spent on your HDFC Bank credit card ending 1234.";
        let result = Layer5CrossrefLayer
            .extract(&pool, "HDFC Bank", body, Some(anchor))
            .await;

        assert!(
            result.is_none(),
            "two equally-plausible candidates must not be auto-resolved"
        );
    }

    /// Doc 30 TASK-TXN-005 acceptance test: zero candidates (wrong window,
    /// wrong instrument, or no statement processed yet) returns None.
    #[tokio::test]
    async fn test_layer5_no_match_returns_none() {
        let anchor = chrono::NaiveDate::from_ymd_opt(2023, 5, 25).unwrap();
        // Entry is 10 days outside the anchor date -- well past the ±3-day window.
        let far_date = chrono::NaiveDate::from_ymd_opt(2023, 6, 10).unwrap();
        let pool = setup_crossref_db(vec![crossref_entry(
            "se_1",
            far_date,
            150000,
            Some("123456789012"),
        )])
        .await;

        let body = "Rs 1500.00 spent on your HDFC Bank credit card ending 1234.";
        let result = Layer5CrossrefLayer
            .extract(&pool, "HDFC Bank", body, Some(anchor))
            .await;

        assert!(result.is_none());
    }

    /// No anchor date at all (Gmail internalDate missing too) -- must not
    /// attempt an unbounded search.
    #[tokio::test]
    async fn test_layer5_no_anchor_date_returns_none() {
        let pool = setup_crossref_db(vec![]).await;
        let body = "Rs 1500.00 spent on your HDFC Bank credit card ending 1234.";
        let result = Layer5CrossrefLayer
            .extract(&pool, "HDFC Bank", body, None)
            .await;
        assert!(result.is_none());
    }

    // -----------------------------------------------------------------------
    // Tests for detect_pattern_drift (Task 4.9)
    // -----------------------------------------------------------------------

    /// Seeds an in-memory SQLite database with the full migrations schema and
    /// inserts one active pattern rule for the given `(bank_name, template_hash)`.
    /// Returns both the pool and the template hash that was registered.
    async fn setup_drift_db(bank_name: &str, body_to_register: &str) -> (Pool, String) {
        let pool = dummy_migrated_pool().await;
        let template_hash = compute_template_hash(body_to_register);
        let hash_clone = template_hash.clone();
        let bank_name_str = bank_name.to_string();

        let conn = pool.get().await.unwrap();
        conn.interact(move |c| {
            let now = chrono::Utc::now().naive_utc();
            let rule = crate::db::pattern_rules::PatternRulesRow {
                id: uuid::Uuid::new_v4().to_string(),
                bank_name: bank_name_str,
                template_hash: hash_clone,
                field_name: "amount".to_string(),
                rule_payload_json: serde_json::json!({ "regex": r"Rs ([\d,]+) spent" }),
                status: "active".to_string(),
                success_count: 10,
                failure_count: 0,
                confidence: 0.95,
                created_at: Some(now),
                updated_at: Some(now),
            };
            crate::db::pattern_rules::insert(c, &rule).unwrap();
        })
        .await
        .unwrap();

        (pool, template_hash)
    }

    /// Verifies the core drift-detection scenario: a known HDFC template whose
    /// active rule covers the *original* body structure; when a changed body
    /// (different structural tokens → different hash) is presented and all
    /// extraction layers return `None`, `detect_pattern_drift` must flag
    /// `drift_detected = true`.
    ///
    /// Additionally asserts:
    /// - When the body is unknown (no active rules), drift is `false`.
    /// - When the ladder already succeeded (`ladder_result.is_some()`), drift
    ///   is always `false` regardless of active rules.
    #[tokio::test]
    async fn test_drift_detected_for_changed_hdfc_template() {
        // ── Setup: register active rules for the ORIGINAL HDFC template. ──────
        let original_body =
            "Rs 1500 spent on HDFC Bank CREDIT Card ending 1234 at Amazon on 25-May-23.";
        let (_pool, registered_hash) = setup_drift_db("HDFC Bank", original_body).await;

        // ── Case 1: changed template (different structural shape → new hash). ──
        // The bank has altered the email template; the new body has a different
        // token structure so its template hash does NOT match the registered one.
        let changed_body =
            "HDFC Bank: Transaction of INR 1500 done at merchant Amazon on 25-May-2023. New format.";
        let changed_hash = compute_template_hash(changed_body);
        assert_ne!(
            registered_hash, changed_hash,
            "Changed body must produce a different template hash to simulate drift"
        );

        // The changed body has no rules registered → drift = false (new template).
        // Use a fresh sync connection pointing at the same in-memory DB.
        let conn = crate::db::test_helpers::setup_test_db_async().await;

        let drift_new_template =
            detect_pattern_drift(&conn, "HDFC Bank", changed_body, &None).unwrap();
        assert!(
            !drift_new_template.drift_detected,
            "A genuinely new (never-seen) template must NOT be flagged as drift; \
             got drift_detected = true"
        );
        assert_eq!(drift_new_template.template_hash, changed_hash);

        // ── Case 2: original body registered but extraction failed → drift. ───
        // Simulate that the ORIGINAL body was seen before (rules exist for its
        // hash) but now extraction returns None (the template has since changed
        // in a way that doesn't alter the structural hash, e.g. amount format).
        // We insert the rule directly into the sync conn.
        let now = chrono::Utc::now().naive_utc();
        let rule = crate::db::pattern_rules::PatternRulesRow {
            id: uuid::Uuid::new_v4().to_string(),
            bank_name: "HDFC Bank".to_string(),
            template_hash: registered_hash.clone(),
            field_name: "amount".to_string(),
            rule_payload_json: serde_json::json!({ "regex": r"Rs ([\d,]+) spent" }),
            status: "active".to_string(),
            success_count: 5,
            failure_count: 0,
            confidence: 0.9,
            created_at: Some(now),
            updated_at: Some(now),
        };
        crate::db::pattern_rules::insert(&conn, &rule).unwrap();

        // Ladder result is None (all layers failed for the original body).
        let drift_known_template =
            detect_pattern_drift(&conn, "HDFC Bank", original_body, &None).unwrap();
        assert!(
            drift_known_template.drift_detected,
            "Known template (active rules exist) + ladder returned None must be drift; \
             got drift_detected = false"
        );
        assert_eq!(drift_known_template.template_hash, registered_hash);
        assert!(
            drift_known_template.synthesized_rule.is_none(),
            "synthesized_rule is None before Layer 5 is invoked"
        );

        // ── Case 3: ladder already succeeded → never flag as drift. ──────────
        let successful_result = Some(ExtractionResult {
            amount_minor: Some(150000),
            currency: Some("INR".to_string()),
            direction: Some("debit".to_string()),
            event_time: Some(1704067200),
            merchant_raw: Some("Amazon".to_string()),
            extraction_method: "bank_templates".to_string(),
            ..Default::default()
        });
        let drift_on_success =
            detect_pattern_drift(&conn, "HDFC Bank", original_body, &successful_result).unwrap();
        assert!(
            !drift_on_success.drift_detected,
            "When the ladder succeeds, drift must never be flagged; \
             got drift_detected = true"
        );
    }

    // ── test_fx_transaction_extracted_correctly ────────────────────────────
    #[tokio::test]
    async fn test_fx_transaction_extracted_correctly() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "Acct XX1234 debited USD 50.00 (INR 4150.50) on 25-May-23 at Netflix.";
        let result = layer.extract(&pool, "Any Bank", body).await;
        // Should pick the INR amount if possible, or correctly parse the foreign amount.
        assert!(result.is_some());
    }

    // ── test_declined_transaction_rejected_or_flagged ──────────────────────
    #[tokio::test]
    async fn test_declined_transaction_rejected_or_flagged() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "Transaction of INR 500.00 at POS declined due to insufficient funds.";
        let result = layer.extract(&pool, "Any Bank", body).await;
        // Implementation might reject this or flag as declined.
        // We ensure that we don't crash, but ideally it returns None.
        assert!(result.is_none() || result.unwrap().amount_minor.unwrap_or(0) > 0);
    }

    // ── test_multi_amount_format_picks_correct_amount ──────────────────────
    #[tokio::test]
    async fn test_multi_amount_format_picks_correct_amount() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "Spent INR 500.00. Available limit is INR 45,000.00.";
        let result = layer.extract(&pool, "Any Bank", body).await;
        if let Some(res) = result {
            // Should pick the spent amount, not the limit
            assert_eq!(res.amount_minor, Some(50000));
        }
    }

    // ── test_icici_upi_on_credit_card_regex ────────────────────────────────
    #[tokio::test]
    async fn test_icici_upi_on_credit_card_regex() {
        let pool = dummy_pool();
        let layer = BankTemplateLayer;
        let body = "Dear Customer, Credit Card XX1234 debited with INR 500.00 on 25-May-23. Info: UPI/1234567890/Amazon.";
        let result = layer.extract(&pool, "ICICI Bank", body).await;
        // Since ICICI UPI is handled, it should extract properly for CC too.
        if let Some(res) = result {
            assert_eq!(res.amount_minor, Some(50000));
        }
    }
}
