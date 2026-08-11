//! The extraction ladder: ordered fallback strategies for one message.
//!
//! Layers run cheapest-first and stop as soon as one produces a result of
//! sufficient confidence. A learned per-bank rule costs nothing and handles the
//! common case; only when no rule matches, or a rule returns implausible
//! output, does the message escalate to the LLM layer.
//!
//! `ExtractionResult` is deliberately all-`Option`: extraction is best-effort,
//! and a partially recovered transaction is more useful than none, so a layer
//! contributes what it can and leaves the rest for the next.
//!
//! Template hashing is what makes learning possible -- messages sharing a
//! structure hash come from the same bank template, so a rule synthesised from
//! one applies to the rest.
use crate::extraction::llm::Layer6Outcome;
use crate::extraction::normalization::clean_masked_identifier;
use anyhow::Result;
use deadpool_sqlite::Pool;
use regex::Regex;
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::future::Future;
use std::pin::Pin;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ExtractionResult {
    pub amount_minor: Option<i64>,
    pub currency: Option<String>,
    pub direction: Option<String>,
    pub event_time: Option<i64>,
    pub merchant_raw: Option<String>,

    pub reference_id: Option<String>,
    pub balance_after: Option<i64>,
    pub original_amount_minor: Option<i64>,
    pub original_currency: Option<String>,

    pub instrument_type: Option<String>,
    pub issuer_name: Option<String>,
    pub masked_identifier: Option<String>,
    pub network: Option<String>,
    pub upi_vpa: Option<String>,

    pub extraction_method: String,
    pub confidence_score: Option<f64>,
    pub parser_version: Option<String>,

    pub emi_total_installments: Option<i32>,
    pub emi_installment_number: Option<i32>,
    pub emi_original_amount_minor: Option<i64>,

    pub exchange_rate: Option<f64>,

    pub event_time_ambiguous: bool,
    pub date_cross_check_flag: Option<String>,
    pub channel: Option<String>,
}

impl ExtractionResult {
    /// Whether this result carries the mandatory fields a transaction needs.
    ///
    /// The ladder's stopping condition: a layer that returns an invalid result has
    /// not really succeeded, so the next layer still runs.
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
    /// Runs this layer against a message.
    ///
    /// Boxed and pinned because the trait is used behind a trait object while some
    /// implementations are async -- a plain async trait method is not object safe.
    fn extract<'a>(
        &'a self,
        pool: &'a Pool,
        bank_name: &'a str,
        body: &'a str,
    ) -> BoxFuture<'a, Option<ExtractionResult>>;
    /// Short identifier recorded on the result, so the producing layer is known.
    fn layer_name(&self) -> &'static str;
}

static TEMPLATE_HASH_DIGITS_RE: OnceLock<Regex> = OnceLock::new();
static TEMPLATE_HASH_WHITESPACE_RE: OnceLock<Regex> = OnceLock::new();

/// Hashes a message's structure, ignoring its variable content.
///
/// The key idea behind learning. Digits are replaced with a placeholder and
/// whitespace collapsed, so every alert generated from one bank template hashes
/// identically regardless of amount, date or merchant. That is what lets a rule
/// synthesised from a single message apply to every future message of the same
/// shape -- and what lets drift be detected when a bank changes its template.
pub fn compute_template_hash(body: &str) -> String {
    let re_digits = TEMPLATE_HASH_DIGITS_RE.get_or_init(|| Regex::new(r"\d+").unwrap());
    let re_whitespace = TEMPLATE_HASH_WHITESPACE_RE.get_or_init(|| Regex::new(r"\s+").unwrap());
    let body_lower = body.to_lowercase();
    let no_digits = re_digits.replace_all(&body_lower, "#");
    let normalized = re_whitespace.replace_all(&no_digits, " ");

    // Trimmed, because collapsing runs of whitespace still leaves a single
    // leading/trailing space when the body had any. MIME-to-text conversion
    // varies that edge whitespace between two renderings of one bank template,
    // and an unstable hash splits a template into several -- which orphans the
    // overrides taught against it and hides drift behind a hash nothing matches.
    let mut hasher = Sha256::new();
    hasher.update(normalized.trim().as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Applies this bank's learned rules to a message.
///
/// The cheapest layer, and the one that handles the common case for free. Runs
/// before any generic parsing or inference is attempted.
pub async fn apply_learned_fields(
    pool: &Pool,
    bank_name: &str,
    body: &str,
    source_type: &str,
    result: &mut ExtractionResult,
) -> bool {
    let (bank, source) = (bank_name.to_string(), source_type.to_string());
    let Ok(conn) = pool.get().await else {
        return false;
    };
    let mut rules = match conn
        .interact(move |c| crate::db::field_rules::select_live_by_bank(c, &bank, &source))
        .await
    {
        Ok(Ok(r)) => r,
        _ => return false,
    };
    if rules.is_empty() {
        return false;
    }

    let body_hash = compute_template_hash(body);

    // `select_live_by_bank` has no ORDER BY, so when several live variants exist
    // for one field the winner would be whatever order SQLite happened to return
    // -- the same message could extract differently between two runs. Rank them,
    // then let the best variant per field be the only one applied.
    rules.sort_by(|a, b| {
        (b.template_hash == body_hash)
            .cmp(&(a.template_hash == body_hash))
            .then(b.confidence.total_cmp(&a.confidence))
            .then(b.success_count.cmp(&a.success_count))
            .then(a.id.cmp(&b.id))
    });

    let mut fired = false;
    let mut applied_fields: Vec<&str> = Vec::new();

    for rule in &rules {
        if applied_fields.contains(&rule.field_name.as_str()) {
            continue;
        }
        let is_override = rule.rule_payload_json.get("override_value").is_some();
        if is_override && rule.template_hash != body_hash {
            continue;
        }
        let Some(captured) =
            crate::extraction::rule_synthesis::apply_payload(&rule.rule_payload_json, body)
        else {
            continue;
        };
        let captured = captured.trim();
        if captured.is_empty() {
            continue;
        }

        match rule.field_name.as_str() {
            "merchant" => result.merchant_raw = Some(captured.to_string()),
            "amount" => {
                if let Some(v) = parse_amount(captured) {
                    result.amount_minor = Some(v);
                } else {
                    continue;
                }
            }
            "balance" => {
                if let Some(v) = parse_amount(captured) {
                    result.balance_after = Some(v);
                } else {
                    continue;
                }
            }
            "reference_id" => result.reference_id = Some(captured.to_string()),
            "last4" => {
                // A capture with neither a digit nor a VPA handle is a
                // mis-synthesised rule, and `clean_masked_identifier` hands back
                // such text unchanged. Written through, it becomes the key of a
                // whole phantom instrument that no real card or account matches
                // -- and it wins over the correctly-read digits, since
                // `apply_instrument_signals` only fills fields still empty.
                let cleaned = clean_masked_identifier(captured);
                if !cleaned.contains('@') && !cleaned.chars().any(|c| c.is_ascii_digit()) {
                    continue;
                }
                result.masked_identifier = Some(cleaned);
            }
            "direction" => match normalize_direction(captured) {
                Some(d) => result.direction = Some(d),
                None => continue,
            },
            "currency" => {
                let normalized = normalize_currency(captured);
                if normalized.len() == 3 && normalized.bytes().all(|b| b.is_ascii_uppercase()) {
                    result.currency = Some(normalized);
                } else {
                    continue;
                }
            }
            "event_time" => match parse_learned_event_time(captured) {
                Some((ts, ambiguous)) => {
                    result.event_time = Some(ts);
                    result.event_time_ambiguous = ambiguous;
                }
                None => continue,
            },
            _ => continue,
        }
        applied_fields.push(rule.field_name.as_str());
        fired = true;
        tracing::debug!(
            bank = bank_name,
            field = %rule.field_name,
            value = %captured,
            "applied a learned extraction rule"
        );
    }

    fired
}

/// Latest epoch second a learned rule may claim as an event time (2100-01-01).
const MAX_PLAUSIBLE_EPOCH_SECONDS: i64 = 4_102_444_800;

/// Earliest epoch second a learned rule may claim as an event time (2000-01-01).
///
/// The mirror of the ceiling, and just as necessary: without a floor, a rule that
/// captures a short numeric token -- an authorisation code, an installment count,
/// a truncated reference -- books the transaction in 1970 rather than being
/// rejected, and a 1970 date is a wrong answer that still looks like a date.
const MIN_PLAUSIBLE_EPOCH_SECONDS: i64 = 946_684_800;

/// Canonicalises a learned direction capture to the two values the ledger allows.
///
/// A rule captures whatever wording its template used -- "debited", "Credited",
/// "DR" -- but every consumer of `direction` compares against exactly "debit" or
/// "credit", so an unnormalised capture is a value nothing downstream recognises.
/// Unrecognised wording yields None so the field is left untouched rather than
/// written as garbage; this mirrors what the statement path already does.
fn normalize_direction(captured: &str) -> Option<String> {
    let c = captured.trim().to_lowercase();
    // "cr"/"dr" are the ledger abbreviations, and they have to match the whole
    // capture. As prefixes they read a direction out of any word that merely
    // begins with those two letters -- a rule capturing "Crest Hotel" books a
    // credit, "Dropbox" a debit -- and a fabricated direction is worse than none,
    // because the field then looks confidently populated.
    let abbrev = c.trim_end_matches('.');
    if abbrev == "cr" || c.contains("credit") || c.contains("received") {
        return Some("credit".to_string());
    }
    if abbrev == "dr"
        || c.contains("debit")
        || c.contains("spent")
        || c.contains("paid")
        || c.contains("withdraw")
    {
        return Some("debit".to_string());
    }
    None
}

/// Reads a learned `event_time` capture as either an epoch or a formatted date.
///
/// A bare `parse::<i64>()` is not enough: bank reference numbers are long digit
/// strings too, and one captured as an event time would book the transaction
/// centuries away instead of being rejected.
fn parse_learned_event_time(captured: &str) -> Option<(i64, bool)> {
    if let Ok(n) = captured.parse::<i64>() {
        if (MIN_PLAUSIBLE_EPOCH_SECONDS..=MAX_PLAUSIBLE_EPOCH_SECONDS).contains(&n) {
            return Some((n, false));
        }
        // Millisecond epochs are read too, but only when the rescaled value is
        // recent: a 12-digit reference number divided by 1000 lands in the 1980s,
        // which is a wrong answer dressed up as a plausible one.
        let as_seconds = n / 1000;
        if (1_000_000_000..=MAX_PLAUSIBLE_EPOCH_SECONDS).contains(&as_seconds) {
            return Some((as_seconds, false));
        }
    }
    parse_date_generic(captured).map(|p| (p.timestamp, p.ambiguous))
}

const LAYER12_CONFIDENCE: f64 = 0.95;

pub struct LearnedFieldLayer;
impl ExtractionLayer for LearnedFieldLayer {
    /// Layer 1: applies rules learned from this bank's previous messages.
    ///
    /// The cheapest layer and the one that handles the steady state, since most mail
    /// a user receives comes from banks the app has already learned.
    fn extract<'a>(
        &'a self,
        pool: &'a Pool,
        bank_name: &'a str,
        body: &'a str,
    ) -> BoxFuture<'a, Option<ExtractionResult>> {
        Box::pin(async move {
            let mut result = ExtractionResult {
                extraction_method: self.layer_name().to_string(),
                confidence_score: Some(LAYER12_CONFIDENCE),
                ..Default::default()
            };
            let fired = apply_learned_fields(pool, bank_name, body, "email", &mut result).await;
            if fired && result.is_valid() {
                Some(result)
            } else {
                None
            }
        })
    }
    /// Identifies results produced by the learned-rule layer.
    fn layer_name(&self) -> &'static str {
        "learned_fields"
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

/// Whether `needle` occurs in `haystack` as a whole word.
///
/// `contains` is the wrong test for short abbreviations: "cc" sits inside
/// "account" and "success", "ecs" inside a name -- enough to label every debit
/// card a credit card and to invent payment channels out of ordinary prose. Both
/// arguments must already be lowercase.
fn contains_word(haystack: &str, needle: &str) -> bool {
    haystack.match_indices(needle).any(|(i, _)| {
        let before = haystack[..i].chars().next_back();
        let after = haystack[i + needle.len()..].chars().next();
        !before.is_some_and(|c| c.is_alphanumeric() || c == '_')
            && !after.is_some_and(|c| c.is_alphanumeric() || c == '_')
    })
}

/// The prefix and suffix currency-amount patterns, compiled once.
///
/// Two are needed because both orderings occur in the wild: `INR 1,200.00` and
/// `1,200.00 INR`. Lazily initialised, since regex compilation is expensive and
/// these run on every message.
///
/// The alphabetic codes are word-anchored: unanchored, `rs` matches inside
/// "Cards 1234" and "Rewards 500", which turns a card number or a loyalty balance
/// into the transaction amount.
fn generic_currency_amount_regexes() -> (&'static Regex, &'static Regex) {
    let prefix = GENERIC_CURRENCY_AMOUNT_PREFIX_RE.get_or_init(|| {
        Regex::new(
            r"(?i)(\brs\.?|\binr|₹|\$|\busd|\beur|\bgbp|\baed|\bsgd|\baud|\bcad|\bjpy|\bchf)\s*([\d,]+(?:\.\d+)?)",
        )
        .unwrap()
    });
    let suffix = GENERIC_CURRENCY_AMOUNT_SUFFIX_RE.get_or_init(|| {
        Regex::new(
            r"(?i)([\d,]+(?:\.\d+)?)\s*(inr\b|rs\.?\b|₹|usd\b|eur\b|gbp\b|aed\b|sgd\b|aud\b|cad\b|jpy\b|chf\b)",
        )
        .unwrap()
    });
    (prefix, suffix)
}

/// Rejects merchant candidates that are not merchants.
///
/// Filters the bank's own name and generic banking vocabulary. Without this, the
/// most common "merchant" in a user's ledger would be their own bank, since its
/// name appears in every alert it sends.
fn is_invalid_merchant(candidate: &str, bank_name: &str) -> bool {
    let re = GENERIC_SELF_REFERENTIAL_MERCHANT_RE
        .get_or_init(|| Regex::new(r"(?i)^(?:your|my|the)\b.*\baccount$|^account$").unwrap());
    if re.is_match(candidate.trim()) {
        return true;
    }

    if crate::extraction::lexicon::is_stopword_only_merchant(candidate.trim()) {
        return true;
    }

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

/// Derives a merchant name from a UPI VPA when nothing better was found.
///
/// A VPA's handle frequently identifies the payee, which is often the only clue
/// a terse UPI alert carries.
fn vpa_merchant_fallback(body: &str) -> Option<String> {
    let re = VPA_MERCHANT_FALLBACK_RE
        .get_or_init(|| Regex::new(r"(?i)\bVPA\s+([\w.\-+]+@[\w.\-]+)").unwrap());
    re.captures(body)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_lowercase().trim_end_matches('.').to_string())
}

static INSTR_CARD_LAST4_RE: OnceLock<Regex> = OnceLock::new();
static INSTR_ACCOUNT_SUFFIX_RE: OnceLock<Regex> = OnceLock::new();
static INSTR_USER_UPI_VPA_DEBIT_RE: OnceLock<Regex> = OnceLock::new();
static INSTR_USER_UPI_VPA_CREDIT_RE: OnceLock<Regex> = OnceLock::new();
static INSTR_USER_UPI_VPA_EXPLICIT_RE: OnceLock<Regex> = OnceLock::new();
static INSTR_CP_UPI_VPA_DEBIT_RE: OnceLock<Regex> = OnceLock::new();
static INSTR_CP_UPI_VPA_CREDIT_RE: OnceLock<Regex> = OnceLock::new();
static INSTR_NETWORK_RE: OnceLock<Regex> = OnceLock::new();

#[derive(Debug, Default, Clone)]
pub struct InstrumentSignals {
    pub instrument_type: Option<String>,
    pub issuer_name: Option<String>,
    pub masked_identifier: Option<String>,
    pub network: Option<String>,
    pub upi_vpa: Option<String>,
}

/// Extracts the clues identifying which account a transaction belongs to.
///
/// Recovers issuer, masked identifier, card network and VPA. These feed
/// attribution, and their absence is precisely what leaves a transaction
/// unassigned rather than being guessed into an arbitrary account.
pub fn extract_instrument_signals(bank_name: &str, body: &str) -> InstrumentSignals {
    let mut signals = InstrumentSignals {
        issuer_name: Some(bank_name.to_string()),
        ..Default::default()
    };

    // The gap between the label and the digits admits a `.` only in the
    // whitespace-free run immediately before them, which is where an ellipsis
    // mask (`...1234`) lives. Allowed anywhere, a sentence-ending full stop
    // bridges "your Credit Card. 25-May-23" and books "25" as the card's last
    // four -- a fabricated identifier that keys a whole phantom instrument.
    const MASK_GAP: &str = r"[Xx*\s\-]*?[Xx*\-.]*?";
    let card_re = INSTR_CARD_LAST4_RE.get_or_init(|| {
        Regex::new(&format!(
            r"(?i)\bcard\b(?:\s+(?:ending|no\.?|number|#|is|in))?\s*(?:with\s+)?{MASK_GAP}(\d{{2,4}})\b"
        ))
        .unwrap()
    });
    if let Some(caps) = card_re.captures(body) {
        if let Some(last4) = caps.get(1) {
            signals.masked_identifier = Some(clean_masked_identifier(last4.as_str()));
            let body_lower = body.to_lowercase();
            // Spelled-out phrases decide before the abbreviations, and the
            // abbreviations are matched as words: "cc" as a substring hits the
            // "account" in every debit-card alert ever sent.
            let kind = if body_lower.contains("credit card") {
                "credit_card"
            } else if body_lower.contains("debit card") {
                "debit_card"
            } else if contains_word(&body_lower, "cc") {
                "credit_card"
            } else if contains_word(&body_lower, "dc") {
                "debit_card"
            } else {
                "credit_card"
            };
            signals.instrument_type = Some(kind.to_string());
        }
    }

    if signals.masked_identifier.is_none() {
        let acc_re = INSTR_ACCOUNT_SUFFIX_RE.get_or_init(|| {
            Regex::new(&format!(
                r"(?i)\b(?:a/c|account|acct)\b(?:\s+(?:ending|no\.?|number|#|is|in))?\s*(?:with\s+)?{MASK_GAP}(\d{{2,4}})\b"
            ))
            .unwrap()
        });
        if let Some(caps) = acc_re.captures(body) {
            if let Some(last4) = caps.get(1) {
                signals.masked_identifier = Some(clean_masked_identifier(last4.as_str()));
                signals.instrument_type = Some("bank_account".to_string());
            }
        }
    }

    let body_lower = body.to_lowercase();
    let is_credit = body_lower.contains("credited")
        || body_lower.contains("received")
        || body_lower.contains("deposited")
        || body_lower.contains("added to")
        || body_lower.contains("refund");

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

    let mut user_vpa_candidates: Vec<String> = Vec::new();

    let user_explicit_re = INSTR_USER_UPI_VPA_EXPLICIT_RE.get_or_init(|| {
        Regex::new(r"(?i)\b(?:your|user)\s+(?:UPI\s+VPA|VPA|UPI\s+ID)\s*:?\s*([\w.\-+]+@[\w.\-]+)")
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
                user_vpa_candidates
                    .push(m.as_str().to_lowercase().trim_end_matches('.').to_string());
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
                user_vpa_candidates
                    .push(m.as_str().to_lowercase().trim_end_matches('.').to_string());
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
        if signals.masked_identifier.is_none() {
            signals.masked_identifier = Some(vpa_str);
            signals.instrument_type = Some("upi_vpa".to_string());
        }
    }

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

/// Writes the recovered instrument signals onto an extraction result.
pub(crate) fn apply_instrument_signals(obs: &mut ExtractionResult, bank_name: &str, body: &str) {
    let signals = extract_instrument_signals(bank_name, body);
    obs.issuer_name = signals.issuer_name;
    obs.instrument_type = obs.instrument_type.take().or(signals.instrument_type);
    obs.masked_identifier = obs.masked_identifier.take().or(signals.masked_identifier);
    obs.network = obs.network.take().or(signals.network);
    obs.upi_vpa = obs.upi_vpa.take().or(signals.upi_vpa);
}

static INTERNAL_TRANSFER_RE: OnceLock<Regex> = OnceLock::new();

/// Infers the payment channel -- UPI, card, NEFT, ATM and so on.
///
/// Uses both the message text and what extraction already established, since the
/// presence of a VPA or a card identifier is itself strong evidence of channel.
pub(crate) fn detect_channel(obs: &ExtractionResult, body: &str) -> Option<String> {
    let internal_transfer_re = INTERNAL_TRANSFER_RE.get_or_init(|| {
        Regex::new(
            r"(?i)credited\s+to\s+(?:the\s+)?account\s+(?:no\.?|number)?\s*(?:ending|no\.?)?\s*[Xx*\-.\s]*\d{2,}",
        )
        .unwrap()
    });
    if internal_transfer_re.is_match(body) {
        return Some("internal_transfer".to_string());
    }

    let lower = body.to_lowercase();
    let has = |w: &str| lower.contains(w);
    // Abbreviations are matched as words -- as substrings they fire on ordinary
    // prose and names, inventing a channel the message never mentioned. "upi" is
    // the exception: it legitimately appears glued to a merchant, as in
    // "UPI_RELIANCE BP MOBILI".
    let has_word = |w: &str| contains_word(&lower, w);

    if has_word("bnpl") || has("buy now pay later") || has("pay later") {
        return Some("bnpl".to_string());
    }
    if has("loan account") || has("loan disbursed") || has("loan emi") {
        return Some("loan".to_string());
    }
    if has_word("ecs") || has_word("nach") {
        return Some("ecs_nach".to_string());
    }
    if has("upi") && obs.instrument_type.as_deref() == Some("credit_card") {
        return Some("upi_credit_card".to_string());
    }
    if has_word("imps") {
        return Some("imps".to_string());
    }
    if has_word("neft") {
        return Some("neft".to_string());
    }
    if has_word("rtgs") {
        return Some("rtgs".to_string());
    }
    if has("upi") {
        return Some("upi".to_string());
    }
    if has_word("pos") {
        return Some("pos".to_string());
    }
    if has_word("atm") {
        return Some("atm".to_string());
    }
    if has("wallet") {
        return Some("wallet".to_string());
    }
    if has("cheque") || has("chq") {
        return Some("cheque".to_string());
    }
    if obs.emi_total_installments.is_some() {
        return Some("emi".to_string());
    }
    None
}

/// Largest amount accepted, in minor units.
///
/// A float-to-int cast saturates instead of wrapping, so without a ceiling a long
/// reference number misread as an amount is booked as `i64::MAX` paise -- a
/// silently absurd figure in the ledger rather than a rejected extraction.
const MAX_PLAUSIBLE_AMOUNT_MINOR: f64 = 1e15;

/// Parses a currency string into integer minor units.
///
/// Returns None rather than zero on failure. A zero amount would be recorded as a
/// real transaction of no value, which is worse than recording nothing.
fn parse_amount(s: &str) -> Option<i64> {
    // A dot straight after a letter belongs to the currency abbreviation, not to
    // the figure: keeping the one in "Rs.2500.00" leaves ".2500.00", which has two
    // decimal points and fails the whole parse, silently dropping the amount. A
    // dot after a digit -- or at the very start, as in ".50" -- is the decimal
    // point and must survive.
    let mut digits_and_dots = String::with_capacity(s.len());
    let mut prev: Option<char> = None;
    for c in s.chars() {
        if c.is_ascii_digit() || (c == '.' && !prev.is_some_and(char::is_alphabetic)) {
            digits_and_dots.push(c);
        }
        prev = Some(c);
    }
    // Bank prose ends amounts with a full stop ("for Rs 706.00.") and the stray
    // trailing dot fails the parse just as surely.
    let minor = digits_and_dots.trim_end_matches('.').parse::<f64>().ok()? * 100.0;
    if !minor.is_finite() || minor > MAX_PLAUSIBLE_AMOUNT_MINOR {
        return None;
    }
    Some(minor.round() as i64)
}

/// Direction assumed when a bank template does not state one.
fn default_pattern_direction() -> String {
    "debit".to_string()
}

/// Currency assumed when a bank template omits it.
fn default_pattern_currency() -> String {
    "INR".to_string()
}

pub const TEMPLATE_TXN_TYPES: &[&str] = &[
    "credit_card",
    "debit_card",
    "upi",
    "account_balance",
    "mandate",
    "emi",
    "atm",
    "net_banking",
    "wallet",
];

#[derive(Debug, serde::Deserialize)]
struct BankPatternTemplate {
    #[allow(dead_code)]
    name: String,
    regex: String,
    amount_group: usize,
    #[serde(default)]
    merchant_group: Option<usize>,
    #[serde(default)]
    date_group: Option<usize>,
    #[serde(default)]
    date_fallback_epoch: Option<i64>,
    #[serde(default = "default_pattern_direction")]
    direction: String,
    #[serde(default)]
    txn_type: Option<String>,
    #[serde(default)]
    balance_group: Option<usize>,
    #[serde(default)]
    reference_group: Option<usize>,
    #[serde(default)]
    last4_group: Option<usize>,
    #[serde(default)]
    upi_vpa_group: Option<usize>,
    #[serde(default)]
    cadence_group: Option<usize>,
    #[serde(default = "default_pattern_currency")]
    currency: String,
}

#[derive(Debug, serde::Deserialize)]
struct BankTemplateFile {
    bank_name: String,
    #[allow(dead_code)]
    version: u32,
    patterns: Vec<BankPatternTemplate>,
}

pub(crate) struct CompiledBankPattern {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) name: String,
    pub(crate) regex: Regex,
    pub(crate) amount_group: usize,
    pub(crate) merchant_group: Option<usize>,
    pub(crate) date_group: Option<usize>,
    pub(crate) date_fallback_epoch: Option<i64>,
    pub(crate) direction: String,
    pub(crate) txn_type: Option<String>,
    pub(crate) balance_group: Option<usize>,
    pub(crate) reference_group: Option<usize>,
    pub(crate) last4_group: Option<usize>,
    pub(crate) upi_vpa_group: Option<usize>,
    pub(crate) cadence_group: Option<usize>,
    pub(crate) currency: String,
}

include!(concat!(env!("OUT_DIR"), "/bank_template_files.rs"));

/// Whether a hand-written template exists for this bank.
pub fn bank_has_template(bank_name: &str) -> bool {
    bank_templates().contains_key(bank_name)
}

/// The template set for a bank, if one is defined.
pub(crate) fn bank_templates(
) -> &'static std::collections::HashMap<String, Vec<CompiledBankPattern>> {
    static TEMPLATES: OnceLock<std::collections::HashMap<String, Vec<CompiledBankPattern>>> =
        OnceLock::new();
    TEMPLATES.get_or_init(|| {
        let mut map = std::collections::HashMap::new();
        for (filename, raw) in BANK_TEMPLATE_FILES {
            let file: BankTemplateFile = serde_json::from_str(raw).unwrap_or_else(|e| {
                panic!("bank_templates/{filename} must parse as a BankTemplateFile: {e}")
            });
            let compiled: Vec<CompiledBankPattern> = file
                .patterns
                .into_iter()
                .map(|p| CompiledBankPattern {
                    regex: Regex::new(&p.regex).unwrap_or_else(|e| {
                        panic!(
                            "bank_templates/{filename} pattern {:?} has an uncompilable regex: {e}",
                            p.name
                        )
                    }),
                    name: p.name,
                    amount_group: p.amount_group,
                    merchant_group: p.merchant_group,
                    date_group: p.date_group,
                    date_fallback_epoch: p.date_fallback_epoch,
                    direction: p.direction,
                    txn_type: p.txn_type,
                    balance_group: p.balance_group,
                    reference_group: p.reference_group,
                    last4_group: p.last4_group,
                    upi_vpa_group: p.upi_vpa_group,
                    cadence_group: p.cadence_group,
                    currency: p.currency,
                })
                .collect();
            map.insert(file.bank_name, compiled);
        }
        map
    })
}

/// Maps a template's transaction type onto an instrument type.
fn instrument_type_for_txn_type(txn_type: &str) -> Option<String> {
    let mapped = match txn_type {
        "credit_card" => "credit_card",
        "debit_card" => "debit_card",
        "upi" => "UPI",
        "atm" => "ATM",
        "wallet" => "wallet",
        "account_balance" | "net_banking" => "bank_account",
        "emi" => "credit_card",
        _ => return None,
    };
    Some(mapped.to_string())
}

pub struct BankTemplateLayer;
impl ExtractionLayer for BankTemplateLayer {
    /// Layer 2: applies a hand-written template for a known bank.
    ///
    /// Covers banks shipped with the app before any learning has occurred, which is
    /// what makes a fresh install useful on day one.
    fn extract<'a>(
        &'a self,
        _pool: &'a Pool,
        bank_name: &'a str,
        body: &'a str,
    ) -> BoxFuture<'a, Option<ExtractionResult>> {
        Box::pin(async move {
            let patterns = bank_templates().get(bank_name)?;
            let mut first_match: Option<ExtractionResult> = None;

            for p in patterns {
                if p.txn_type.as_deref() == Some("mandate") {
                    continue;
                }
                let Some(caps) = p.regex.captures(body) else {
                    continue;
                };

                // Built fresh per pattern: a shared accumulator would carry a
                // previous pattern's fields into this one's result.
                let mut result = ExtractionResult {
                    extraction_method: "bank_templates".to_string(),
                    confidence_score: Some(LAYER12_CONFIDENCE),
                    direction: Some(p.direction.clone()),
                    currency: Some(p.currency.clone()),
                    ..Default::default()
                };
                result.amount_minor = caps
                    .get(p.amount_group)
                    .and_then(|m| parse_amount(m.as_str()));
                result.merchant_raw = p
                    .merchant_group
                    .and_then(|g| caps.get(g))
                    .map(|m| m.as_str().trim().to_string())
                    .filter(|m| !m.is_empty());
                let parsed_date = p
                    .date_group
                    .and_then(|g| caps.get(g))
                    .and_then(|m| parse_date_generic(m.as_str()));
                result.event_time_ambiguous = parsed_date.as_ref().is_some_and(|d| d.ambiguous);
                result.event_time = parsed_date.map(|d| d.timestamp).or(p.date_fallback_epoch);
                result.balance_after = p
                    .balance_group
                    .and_then(|g| caps.get(g))
                    .and_then(|m| parse_amount(m.as_str()));
                result.reference_id = p
                    .reference_group
                    .and_then(|g| caps.get(g))
                    .map(|m| m.as_str().trim().to_string())
                    .filter(|r| !r.is_empty());
                result.masked_identifier = p
                    .last4_group
                    .and_then(|g| caps.get(g))
                    .map(|m| clean_masked_identifier(m.as_str()))
                    .filter(|d| !d.is_empty());
                result.upi_vpa = p
                    .upi_vpa_group
                    .and_then(|g| caps.get(g))
                    .map(|m| m.as_str().trim().to_lowercase())
                    .filter(|v| !v.is_empty());
                result.instrument_type =
                    p.txn_type.as_deref().and_then(instrument_type_for_txn_type);

                // Stopping at the first pattern that merely *matched* threw the
                // layer away whenever a loose pattern matched first and produced
                // an unusable result -- the later, tighter pattern that would
                // have completed the transaction never got to run.
                if result.is_valid() {
                    return Some(result);
                }
                if first_match.is_none() {
                    first_match = Some(result);
                }
            }

            first_match
        })
    }
    /// Identifies results produced by the bank-template layer.
    fn layer_name(&self) -> &'static str {
        "bank_templates"
    }
}

const LAYER3_BASE_CONFIDENCE: f64 = 0.5;
const LAYER3_MAX_CONFIDENCE: f64 = 0.7;
const LAYER3_AMOUNT_CURRENCY_BONUS: f64 = 0.10;
const LAYER3_EXPLICIT_DIRECTION_BONUS: f64 = 0.10;
const LAYER3_STRICT_MERCHANT_BONUS: f64 = 0.15;
const LAYER3_AMBIGUOUS_MERCHANT_BONUS: f64 = 0.05;
const LAYER3_REFERENCE_ID_BONUS: f64 = 0.05;

pub struct GenericRegexLayer;
impl ExtractionLayer for GenericRegexLayer {
    /// Layer 3: generic currency and date patterns, with no bank knowledge.
    ///
    /// The fallback for institutions with neither a template nor learned rules.
    /// Recovers amount and date reliably; merchant far less so.
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

            let (prefix_re, suffix_re) = generic_currency_amount_regexes();

            // `?` here would abandon the whole layer on a group that did not
            // participate, discarding the fields already recovered.
            if let Some(caps) = prefix_re.captures(body) {
                result.currency = caps.get(1).map(|m| normalize_currency(m.as_str()));
                result.amount_minor = caps.get(2).and_then(|m| parse_amount(m.as_str()));
            } else if let Some(caps) = suffix_re.captures(body) {
                result.amount_minor = caps.get(1).and_then(|m| parse_amount(m.as_str()));
                result.currency = caps.get(2).map(|m| normalize_currency(m.as_str()));
            }

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

            const MERCHANT_TERMINATOR: &str = r":?\s+([A-Za-z0-9\s*_&'./@-]{2,40}?)(?:\s+on\b|\s+via\b|\s+using\b|\s+with\b|\s+ref\b|\s+card\b|\s+date\b|\s+a/c\b|\s+branch\b|\s+upi\b|\s+is\b|\s+was\b|[,.\n\-]|$)";
            let merchant_re_strict = GENERIC_MERCHANT_RE_STRICT.get_or_init(|| {
                let alternation = crate::extraction::lexicon::MERCHANT_LABEL_STRICT.join("|");
                Regex::new(&format!(r"(?i)\b(?:{alternation}){MERCHANT_TERMINATOR}")).unwrap()
            });
            let merchant_re = GENERIC_MERCHANT_RE.get_or_init(|| {
                let alternation = crate::extraction::lexicon::MERCHANT_LABEL_AMBIGUOUS.join("|");
                Regex::new(&format!(r"(?i)\b(?:{alternation}){MERCHANT_TERMINATOR}")).unwrap()
            });
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
            if merchant_value.is_none() {
                merchant_value = vpa_merchant_fallback(body);
            }
            result.merchant_raw = merchant_value;

            let date_re = GENERIC_DATE_RE.get_or_init(|| {
                Regex::new(r"(?i)(\d{2}[-/]\d{2}[-/]\d{2,4}|\d{2}-[a-zA-Z]{3}-\d{2,4}|\d{2}\s+[a-zA-Z]{3},?\s+\d{2,4}|[a-zA-Z]{3}\s+\d{2},\s*\d{4})").unwrap()
            });
            // The first date-shaped span is not necessarily a parseable date
            // ("99/99/9999"); keep looking rather than giving up on the message.
            if let Some(parsed) = date_re
                .captures_iter(body)
                .filter_map(|c| c.get(1))
                .find_map(|m| parse_date_generic(m.as_str()))
            {
                result.event_time = Some(parsed.timestamp);
                result.event_time_ambiguous = parsed.ambiguous;
            }

            let ref_re = GENERIC_REF_RE.get_or_init(|| Regex::new(r"\b(\d{12})\b").unwrap());
            result.reference_id = ref_re
                .captures(body)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string());

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
    /// Identifies results produced by the generic-regex layer.
    fn layer_name(&self) -> &'static str {
        "generic_regex"
    }
}

/// Canonicalises a currency token to an ISO code.
///
/// Indian banks write rupees as `Rs`, `Rs.`, `INR` and the symbol
/// interchangeably; without normalisation these would be treated as four
/// different currencies.
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

#[derive(Debug, PartialEq)]
struct DateParseResult {
    timestamp: i64,
    ambiguous: bool,
}

const NUMERIC_AMBIGUOUS_FORMATS: &[&str] =
    &["%d/%m/%Y", "%d-%m-%Y", "%m-%d-%Y", "%d/%m/%y", "%d-%m-%y"];

/// Parses a date in whichever format the bank used.
///
/// Reports how the date was interpreted alongside the value, because
/// day-first and month-first formats are genuinely ambiguous below the
/// thirteenth -- the caller needs that uncertainty rather than a confident guess.
fn parse_date_generic(s: &str) -> Option<DateParseResult> {
    let formats = [
        "%d-%b-%y",
        "%d-%b-%Y",
        "%d/%m/%y",
        "%d/%m/%Y",
        "%d-%m-%y",
        "%d-%m-%Y",
        "%m-%d-%Y",
        "%d %b %y",
        "%d %b %Y",
        "%d %b, %y",
        "%d %b, %Y",
        "%b %d, %Y",
        "%a, %b %d, %Y",
        "%a, %B %d, %Y",
        "%d %B %Y",
        "%d %B, %Y",
        "%B %d, %Y",
        // Last, so "23-12-25" is still read day-first rather than as year 23.
        // These are the shapes the learning path itself writes back as a
        // corrected event_time, so without them a learned date rule can never
        // re-apply.
        "%Y-%m-%d",
        "%Y-%m-%d %H:%M:%S",
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

/// Collects the tokens around a matched label as a merchant candidate.
///
/// Merchant names are unbounded, so the window is captured and then trimmed,
/// rather than trying to express the whole name in one pattern.
fn collect_merchant_window(
    tokens: &[&str],
    lower_tokens: &[String],
    start: usize,
) -> Option<String> {
    let mut merchant_parts = Vec::new();
    let mut j = start;
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

/// Pulls the payee out of a UPI narration such as `UPI/1234567890/AmazonPay`.
///
/// The whole narration is a worse merchant than the name inside it, and both the
/// label scan and the token scan can surface it, so what counts as the payee is
/// defined once here.
fn upi_narration_payee(candidate: &str) -> Option<String> {
    if !candidate.to_lowercase().contains("upi/") {
        return None;
    }
    let parts: Vec<&str> = candidate.split('/').collect();
    let payee = parts.get(2)?.trim_end_matches(&['.', ','][..]).trim();
    (!payee.is_empty()).then(|| payee.to_string())
}

pub struct NlpLayer;
impl ExtractionLayer for NlpLayer {
    /// Layer 4: label-driven token scanning for fields the regexes missed.
    ///
    /// Works from field captions rather than value shapes, which is what lets it find
    /// a merchant name -- a value with no distinctive form of its own.
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
                        let candidate = upi_narration_payee(&candidate).unwrap_or(candidate);
                        if !is_invalid_merchant(&candidate, bank_name) {
                            strict_merchant_candidate = Some(candidate);
                            break;
                        }
                    }
                }
            }

            // A strict label ("towards X", "merchant: X") is the more reliable
            // signal, so it wins outright rather than only filling in when the
            // ambiguous "at/to/from" scan below found nothing -- which is the
            // precedence Layer 3 already applies.
            result.merchant_raw = strict_merchant_candidate;

            let mut i = 0;
            while i < tokens.len() {
                let token = &lower_tokens[i];
                let orig_token = tokens[i];

                // First verb wins: read to the end and the closing disclaimer
                // ("if this credit was not authorised...") flips the direction of
                // a transaction the message already stated.
                if result.direction.is_none() {
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
                }

                if (token == "rs" || token == "rs." || token == "inr" || token == "₹")
                    && i + 1 < tokens.len()
                    && result.amount_minor.is_none()
                {
                    if let Some(amt) = parse_amount(tokens[i + 1]) {
                        result.amount_minor = Some(amt);
                        result.currency = Some("INR".to_string());
                    }
                }

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
                    // The same window the strict-label scan collects, and it must
                    // stay the same: two copies of this loop drifted apart once
                    // already, so the ambiguous tier stopped unwrapping the UPI
                    // narration the strict tier did.
                    if let Some(candidate) = collect_merchant_window(&tokens, &lower_tokens, i + 1)
                    {
                        let candidate = upi_narration_payee(&candidate).unwrap_or(candidate);
                        if !is_invalid_merchant(&candidate, bank_name) {
                            result.merchant_raw = Some(candidate);
                        }
                    }
                }

                if result.merchant_raw.is_none() {
                    if let Some(candidate) = upi_narration_payee(orig_token) {
                        if !is_invalid_merchant(&candidate, bank_name) {
                            result.merchant_raw = Some(candidate);
                        }
                    }
                }

                if token == "bal"
                    || token == "balance"
                    || token.starts_with("bal:")
                    || token.starts_with("balance:")
                    || token == "avl"
                {
                    // "Bal:1000.00" carries the value in the same token; looking
                    // only at the next one reads past it. First balance wins, as
                    // everywhere else in this layer -- unguarded, a trailing
                    // "Reward Bal: 0" overwrote the account balance already read.
                    if result.balance_after.is_none() {
                        if let Some((_, inline)) = orig_token.split_once(':') {
                            if let Some(amt) = parse_amount(inline) {
                                result.balance_after = Some(amt);
                            }
                        }
                    }
                    let mut j = i + 1;
                    if token == "avl" && j < tokens.len() && lower_tokens[j] == "bal" {
                        j += 1;
                    }
                    // A run, not one token: "Avl Bal is Rs 100" puts two fillers
                    // between the label and the figure, and skipping only one
                    // leaves the parse pointed at "Rs".
                    while j < tokens.len()
                        && (lower_tokens[j] == "rs"
                            || lower_tokens[j] == "rs."
                            || lower_tokens[j] == "inr"
                            || lower_tokens[j] == "₹"
                            || lower_tokens[j] == "-"
                            || lower_tokens[j] == "is")
                    {
                        j += 1;
                    }
                    if j < tokens.len() && result.balance_after.is_none() {
                        if let Some(amt) = parse_amount(tokens[j]) {
                            result.balance_after = Some(amt);
                        }
                    }
                }

                // First date wins, for the same reason as direction: a later "on
                // <date>" in the footer must not replace the transaction's own.
                if token == "on" && i + 1 < tokens.len() && result.event_time.is_none() {
                    let dt_str = tokens[i + 1].trim_end_matches(&['.', ','][..]);
                    if let Some(parsed) = parse_date_generic(dt_str) {
                        result.event_time = Some(parsed.timestamp);
                        result.event_time_ambiguous = parsed.ambiguous;
                    }
                }

                i += 1;
            }

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
    /// Identifies results produced by the NLP layer.
    fn layer_name(&self) -> &'static str {
        "nlp"
    }
}

#[derive(Debug, Clone)]
pub struct DriftResult {
    pub drift_detected: bool,
    pub template_hash: String,
}

/// Detects that a bank has changed a template this app had learned.
///
/// The signal is the combination of two facts: extraction produced nothing, yet
/// rules exist for this exact template hash. Rules that used to match and no
/// longer do means the template moved underneath them.
///
/// A successful extraction is never drift, which is why that case returns early.
pub fn detect_pattern_drift(
    conn: &Connection,
    bank_name: &str,
    body: &str,
    ladder_result: &Option<ExtractionResult>,
) -> Result<DriftResult> {
    let template_hash = compute_template_hash(body);

    if ladder_result.is_some() {
        return Ok(DriftResult {
            drift_detected: false,
            template_hash,
        });
    }

    let known_rule_count = crate::db::field_rules::count_live_by_bank_and_hash(
        conn,
        bank_name,
        &template_hash,
        "email",
    )?;

    Ok(DriftResult {
        drift_detected: known_rule_count > 0,
        template_hash,
    })
}

/// Confidence for a result completed from a parsed statement.
///
/// High, and deliberately above the auto-resolve threshold: the fields come from
/// the bank's own statement, matched uniquely on instrument, date window and
/// amount or reference. Leaving it unset made every crossref result read as
/// "unknown confidence", which downstream treats as not confident at all.
const LAYER5_CONFIDENCE: f64 = 0.9;

pub struct Layer5CrossrefLayer;

impl Layer5CrossrefLayer {
    /// Layer 5: fills gaps by cross-referencing already-ingested data.
    ///
    /// Uses the user's own history rather than the message: a merchant seen before on
    /// the same instrument supplies what a terse alert omitted. Cheaper and more
    /// reliable than inference, which is why it precedes the LLM.
    pub async fn extract(
        &self,
        pool: &Pool,
        bank_name: &str,
        body: &str,
        anchor_date: Option<chrono::NaiveDate>,
    ) -> Option<ExtractionResult> {
        let anchor_date = anchor_date?;

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
            confidence_score: Some(LAYER5_CONFIDENCE),
            ..Default::default()
        })
    }
}

pub struct Layer6LlmLayer {
    pub app_dir: Option<std::path::PathBuf>,
    pub fallback_event_time: Option<i64>,
}
impl Layer6LlmLayer {
    /// Executes the LLM call and classifies its outcome.
    ///
    /// Separated from the trait method so the outcome -- success, refusal, malformed
    /// output -- is available to callers that need to distinguish them, rather than
    /// being flattened into an Option.
    pub(crate) async fn run(&self, pool: &Pool, bank_name: &str, body: &str) -> Layer6Outcome {
        let app_dir = match &self.app_dir {
            Some(dir) => dir,
            None => {
                tracing::warn!("Layer 6: No app_dir provided, cannot locate LLM model");
                return Layer6Outcome::Failed;
            }
        };

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
        let Some(model_id) =
            crate::llm_manager::resolve_active_model(&downloaded, stored.as_deref())
        else {
            tracing::warn!("Layer 6: No downloaded LLM model available");
            return Layer6Outcome::Failed;
        };

        tracing::info!(bank_name = bank_name, "Layer 6 (LLM) extraction invoked");

        let engine = crate::extraction::llm::LlmEngine::new(app_dir, &model_id);
        let result = engine
            .extract(bank_name, body, self.fallback_event_time)
            .await;

        tracing::info!(
            event = "layer6_usage",
            bank_name = bank_name,
            success = matches!(result, Layer6Outcome::Extracted(_)),
            "Layer 6 fallback utilized"
        );

        result
    }
}
impl ExtractionLayer for Layer6LlmLayer {
    /// Layer 6: local LLM inference, the last and most expensive resort.
    ///
    /// Reached only when every cheaper layer failed. Output is schema-constrained and
    /// then validated against the source, because a confidently wrong amount is
    /// exactly what must not reach a financial ledger.
    fn extract<'a>(
        &'a self,
        pool: &'a Pool,
        bank_name: &'a str,
        body: &'a str,
    ) -> BoxFuture<'a, Option<ExtractionResult>> {
        Box::pin(async move {
            match self.run(pool, bank_name, body).await {
                Layer6Outcome::Extracted(result) => Some(*result),
                Layer6Outcome::TimedOut | Layer6Outcome::Failed | Layer6Outcome::Rejected => None,
            }
        })
    }
    /// Identifies results produced by the LLM layer.
    fn layer_name(&self) -> &'static str {
        "llm_layer6"
    }
}

#[derive(Debug, PartialEq)]
enum AmountAgreement {
    Agrees,
    Disagrees,
    Inconclusive,
}

/// Independently re-reads the amount from the message and compares it.
///
/// A second opinion on the single most consequential field. The generic currency
/// patterns are deliberately used here rather than the layer that produced the
/// value, so the check is genuinely independent rather than confirming the same
/// logic twice.
///
/// Three outcomes, not two: finding no amount is inconclusive rather than
/// disagreement, since many messages state the amount only once.
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

const CROSS_CHECK_DISAGREEMENT_CONFIDENCE: f64 = 0.4;
const CROSS_CHECK_DISAGREEMENT_PENALTY_FACTOR: f64 = 0.8;

/// Downgrades confidence when the independent amount read disagrees.
///
/// Confidence is reduced rather than the transaction rejected. A disagreement
/// means uncertainty, not proof of error, and discarding a real transaction is a
/// worse outcome than recording one that is flagged for review.
fn apply_amount_cross_check(obs: &mut ExtractionResult, body: &str) {
    let Some(claimed) = obs.amount_minor else {
        return;
    };
    // On a foreign-currency transaction the first currency-amount in the body is
    // the foreign one, so checking only the settled amount marked every single FX
    // transaction as a disagreement.
    let foreign_agrees = obs
        .original_amount_minor
        .is_some_and(|orig| cross_check_amount(body, orig) == AmountAgreement::Agrees);
    if !foreign_agrees && cross_check_amount(body, claimed) == AmountAgreement::Disagrees {
        let downgraded = match obs.confidence_score {
            Some(existing) => (existing * CROSS_CHECK_DISAGREEMENT_PENALTY_FACTOR)
                .clamp(0.0, CROSS_CHECK_DISAGREEMENT_CONFIDENCE),
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

const DATE_CROSS_CHECK_DECISIVE_RATIO: i64 = 3;
const DATE_CROSS_CHECK_PLAUSIBLE_DELAY_DAYS: i64 = 7;

/// Sanity-checks the extracted date against the message's own timestamp.
///
/// An event time far from when the mail arrived usually means a misparsed date --
/// most often a day/month swap -- so the divergence lowers confidence.
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
    let Some(anchor) =
        chrono::DateTime::from_timestamp(anchor_ts, 0).map(|dt| dt.naive_utc().date())
    else {
        return;
    };
    let (day, month, year) = (original.day(), original.month(), original.year());
    if day == month {
        return;
    }
    let Some(swapped) = chrono::NaiveDate::from_ymd_opt(year, day, month) else {
        return;
    };

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

/// Enriches and cross-checks whichever layer's result won.
///
/// One function rather than per-path copies, because the copies drifted: the
/// crossref and LLM results were skipping channel, EMI, FX and the date sanity
/// check entirely, and the LLM path ran its amount cross-check *before* learned
/// rules could still change the amount, checking a value it then discarded.
async fn finalize_result(
    pool: &Pool,
    bank_name: &str,
    body: &str,
    internal_date: Option<i64>,
    obs: &mut ExtractionResult,
) {
    // Layer 1 is itself nothing but learned rules; re-running them would be a
    // second query and a second round of identical log lines.
    if obs.extraction_method != "learned_fields" {
        apply_learned_fields(pool, bank_name, body, "email", obs).await;
    }
    apply_instrument_signals(obs, bank_name, body);
    if let Some((number, total)) =
        crate::extraction::emi_detector::detect_emi_installment_numbers(body)
    {
        obs.emi_installment_number = Some(number);
        obs.emi_total_installments = Some(total);
        obs.emi_original_amount_minor =
            crate::extraction::emi_detector::detect_emi_original_amount_minor(body);
    }
    let settled_currency = obs.currency.clone().unwrap_or_else(|| "INR".to_string());
    let fx = crate::extraction::currency_handler::detect_fx_fields(body, &settled_currency);
    // Filled in, not overwritten: the LLM layer can report FX fields this
    // regex-based detector does not see.
    obs.original_amount_minor = obs
        .original_amount_minor
        .take()
        .or(fx.original_amount_minor);
    obs.original_currency = obs.original_currency.take().or(fx.original_currency);
    obs.exchange_rate = obs.exchange_rate.take().or(fx.exchange_rate);
    obs.channel = detect_channel(obs, body);
    apply_amount_cross_check(obs, body);
    apply_date_cross_check(obs, internal_date);
}

#[allow(clippy::too_many_arguments)]
/// Runs the extraction layers in order, stopping at the first sufficient result.
///
/// The coordinator of the whole module. Layers are ordered cheapest-first --
/// learned rules, bank templates, generic regex, NLP, cross-reference, and
/// finally the LLM -- so the expensive path is reached only for messages the
/// cheap ones could not handle.
///
/// Cross-checks are applied to whatever result emerges, regardless of which layer
/// produced it, so a confident answer from an expensive layer is still verified.
pub async fn run_extraction_ladder(
    pool: &Pool,
    bank_name: &str,
    body: &str,
    app_dir: Option<std::path::PathBuf>,
    llm_eligible: bool,
    internal_date: Option<i64>,
    layer6_timed_out: &mut bool,
    learning: Option<&crate::learning::LearningHandle>,
) -> Result<Option<ExtractionResult>> {
    let layers: [&dyn ExtractionLayer; 4] = [
        &LearnedFieldLayer,
        &BankTemplateLayer,
        &GenericRegexLayer,
        &NlpLayer,
    ];

    for layer in layers {
        let layer_name = layer.layer_name();
        if let Some(mut obs) = layer.extract(pool, bank_name, body).await {
            if obs.is_valid() {
                finalize_result(pool, bank_name, body, internal_date, &mut obs).await;
                tracing::info!(
                    layer = layer_name,
                    status = "success",
                    "Extraction layer succeeded"
                );
                return Ok(Some(obs));
            }
        }
        if layer_name == "learned_fields" {
            tracing::debug!(
                layer = layer_name,
                status = "no_rules",
                "Extraction layer skipped (no learned rules yet)"
            );
        } else if layer_name == "bank_templates" && !bank_has_template(bank_name) {
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

    let anchor_date = internal_date
        .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0).map(|dt| dt.naive_utc().date()));
    if let Some(mut crossref_result) = Layer5CrossrefLayer
        .extract(pool, bank_name, body, anchor_date)
        .await
    {
        if crossref_result.is_valid() {
            finalize_result(pool, bank_name, body, internal_date, &mut crossref_result).await;
            tracing::info!(
                layer = "layer5_statement_crossref",
                status = "success",
                "Extraction layer succeeded"
            );
            return Ok(Some(crossref_result));
        }
    }
    tracing::info!(
        layer = "layer5_statement_crossref",
        status = "failure",
        "Extraction layer failed"
    );

    if !llm_eligible {
        tracing::info!(
            bank_name = bank_name,
            "Layer 6 skipped — LLM not RAM-eligible"
        );
        return Ok(None);
    }

    let layer6 = Layer6LlmLayer {
        app_dir: app_dir.clone(),
        fallback_event_time: internal_date,
    };
    let layer6_outcome = layer6.run(pool, bank_name, body).await;
    if matches!(layer6_outcome, Layer6Outcome::TimedOut) {
        *layer6_timed_out = true;
    }
    if let Layer6Outcome::Extracted(boxed_llm_result) = layer6_outcome {
        let mut llm_result = *boxed_llm_result;
        if llm_result.is_valid() {
            finalize_result(pool, bank_name, body, internal_date, &mut llm_result).await;

            if let Some(handle) = learning {
                enqueue_drift_candidates_if_drifted(
                    pool,
                    handle,
                    bank_name,
                    body,
                    &llm_result,
                    app_dir.clone(),
                )
                .await;
            }

            return Ok(Some(llm_result));
        }
    }

    Ok(None)
}

/// Queues rule re-synthesis when drift was detected.
///
/// Gated on drift so the expensive authoring path runs only when a bank actually
/// changed something, rather than on every failed extraction.
pub async fn enqueue_drift_candidates_if_drifted(
    pool: &Pool,
    learning: &crate::learning::LearningHandle,
    bank_name: &str,
    body: &str,
    result: &ExtractionResult,
    app_dir: Option<std::path::PathBuf>,
) {
    let b_name = bank_name.to_string();
    let body_owned = body.to_string();
    let drift = {
        let Ok(conn) = pool.get().await else { return };
        conn.interact(move |c| detect_pattern_drift(c, &b_name, &body_owned, &None))
            .await
        // The pooled connection is released here rather than being held across
        // the enqueue below, which needs no database of its own.
    };
    let Ok(Ok(drift)) = drift else { return };
    if !drift.drift_detected {
        return;
    }
    tracing::warn!(
        bank_name = bank_name,
        template_hash = %drift.template_hash,
        "Template drift detected — queueing replacement rules."
    );
    enqueue_drift_candidates(learning, bank_name, body, result, app_dir).await;
}

/// Queues the messages that will be used to author replacement rules.
async fn enqueue_drift_candidates(
    handle: &crate::learning::LearningHandle,
    bank_name: &str,
    body: &str,
    result: &ExtractionResult,
    app_dir: Option<std::path::PathBuf>,
) {
    let fields: Vec<(&str, String)> = [
        result.merchant_raw.clone().map(|v| ("merchant", v)),
        result.amount_minor.map(|v| ("amount", v.to_string())),
        result.reference_id.clone().map(|v| ("reference_id", v)),
        result.balance_after.map(|v| ("balance", v.to_string())),
        result.event_time.and_then(|ts| {
            chrono::DateTime::from_timestamp(ts, 0).map(|dt| {
                (
                    "event_time",
                    dt.naive_utc().format("%Y-%m-%d %H:%M:%S").to_string(),
                )
            })
        }),
    ]
    .into_iter()
    .flatten()
    .collect();

    for (field_name, new_value) in fields {
        crate::learning::enqueue(
            handle,
            crate::learning::FeedbackJob {
                feedback_log_id: String::new(),
                bank_name: bank_name.to_string(),
                field_name: field_name.to_string(),
                source_type: "email".to_string(),
                source_text: body.to_string(),
                old_value: None,
                new_value,
                observation_id: None,
                learned_from: "drift_llm".to_string(),
                app_dir: app_dir.clone(),
            },
        )
        .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bank_template_integrity() {
        let templates = bank_templates();
        assert!(
            !templates.is_empty(),
            "no bank templates compiled -- build.rs glob produced an empty set"
        );

        for (bank, patterns) in templates {
            assert!(
                !patterns.is_empty(),
                "{bank}: template file has zero patterns"
            );
            for p in patterns {
                let groups = p.regex.captures_len();
                let named = [
                    ("amount_group", Some(p.amount_group)),
                    ("merchant_group", p.merchant_group),
                    ("date_group", p.date_group),
                    ("balance_group", p.balance_group),
                    ("reference_group", p.reference_group),
                    ("last4_group", p.last4_group),
                    ("upi_vpa_group", p.upi_vpa_group),
                    ("cadence_group", p.cadence_group),
                ];
                for (field, group) in named {
                    if let Some(g) = group {
                        assert!(
                            g > 0 && g < groups,
                            "{bank}/{}: {field}={g} but the regex has {} capture groups \
                             (valid indices 1..{}) -- this field would silently never populate",
                            p.name,
                            groups - 1,
                            groups - 1
                        );
                    }
                }
                assert!(
                    p.direction == "debit" || p.direction == "credit",
                    "{bank}/{}: direction {:?} must be \"debit\" or \"credit\"",
                    p.name,
                    p.direction
                );
                if let Some(t) = p.txn_type.as_deref() {
                    assert!(
                        TEMPLATE_TXN_TYPES.contains(&t),
                        "{bank}/{}: txn_type {t:?} is not one of {TEMPLATE_TXN_TYPES:?}",
                        p.name
                    );
                }
                assert!(
                    p.currency.len() == 3 && p.currency.chars().all(|c| c.is_ascii_uppercase()),
                    "{bank}/{}: currency {:?} must be a 3-letter uppercase ISO code",
                    p.name,
                    p.currency
                );
                if p.txn_type.as_deref() == Some("mandate") {
                    assert!(
                        p.merchant_group.is_some(),
                        "{bank}/{}: a mandate pattern needs a merchant_group",
                        p.name
                    );
                }
                if p.txn_type.as_deref() != Some("account_balance")
                    && p.txn_type.as_deref() != Some("mandate")
                {
                    assert!(
                        p.merchant_group.is_some() || p.balance_group.is_some(),
                        "{bank}/{}: no merchant_group and no balance_group -- \
                         `ExtractionResult::is_valid()` can never pass for this pattern",
                        p.name
                    );
                }
            }
        }
    }

    #[tokio::test]
    async fn bank_template_confidence_outranks_generic_regex() {
        let pool = dummy_pool();
        let got = BankTemplateLayer
            .extract(
                &pool,
                "Jupiter",
                "Hey, Aditya Your UPI payment was successful You paid ₹543 Paid to \
                 HONGKONG NOODLES Vyapar.169687998887@hdfcbank Date Jan 01, 2026 From \
                 Aditya 8127696200@jupiteraxis Transaction ID 1321767280821724605",
            )
            .await
            .expect("template must still match");

        let confidence = got
            .confidence_score
            .expect("Doc 30 TASK-TXN-004 gives Layers 1/2 a band; NULL is not it");
        assert!(
            confidence > LAYER3_MAX_CONFIDENCE,
            "a template match ({confidence}) must outrank the best possible \
             generic-regex result ({LAYER3_MAX_CONFIDENCE}), or precedence \
             cannot prefer it"
        );
        assert!(confidence >= 0.9, "Doc 30 says Layer 1/2 is typically 0.9+");
    }

    #[tokio::test]
    async fn test_tier1_templates_extract_real_bodies() {
        struct Case {
            bank: &'static str,
            body: &'static str,
            amount_minor: i64,
            direction: &'static str,
            merchant: Option<&'static str>,
            last4: Option<&'static str>,
            date: Option<(i32, u32, u32)>,
            balance_minor: Option<i64>,
        }

        let cases = [
            Case {
                bank: "HDFC Bank",
                body: "Dear Customer, Rs.200.00 has been debited from account 4691 to VPA \
                       shreesomnathtrustvas.76061863@hdfcbank SHREE SOMNATH TRUST VAS on \
                       23-12-25. Your UPI transaction reference number is 533264925852.",
                amount_minor: 20000,
                direction: "debit",
                merchant: Some("SHREE SOMNATH TRUST VAS"),
                last4: Some("4691"),
                date: Some((2025, 12, 23)),
                balance_minor: None,
            },
            Case {
                bank: "HDFC Bank",
                body: "Dear Card Member, Thank you for using your HDFC Bank Credit Card ending \
                       0364 for Rs 706.00 at Payu*Swiggy Food on 07-08-2025 19:25:29. \
                       Authorization code:- 002587",
                amount_minor: 70600,
                direction: "debit",
                merchant: Some("Payu*Swiggy Food"),
                last4: Some("0364"),
                date: Some((2025, 8, 7)),
                balance_minor: None,
            },
            Case {
                bank: "HDFC Bank",
                body: "Dear Customer, Here is the update on your account balance: As of \
                       yesterday, 04-SEP-25 available balance is INR 10050.00 in your A/c XX4691",
                amount_minor: 1005000,
                direction: "credit",
                merchant: None,
                last4: Some("4691"),
                date: Some((2025, 9, 4)),
                balance_minor: Some(1005000),
            },
            Case {
                bank: "SBI Card",
                body: "SBI Card TRANSACTION ALERT! Dear Cardholder, This is to inform you that, \
                       Rs.480.20 spent on your SBI Credit Card ending 7603 at \
                       INNOVATIVERETAILCONCEPT on 10/01/26.",
                amount_minor: 48020,
                direction: "debit",
                merchant: Some("INNOVATIVERETAILCONCEPT"),
                last4: Some("7603"),
                date: Some((2026, 1, 10)),
                balance_minor: None,
            },
            Case {
                bank: "IDFC FIRST Bank",
                body: "Dear Cardmember, Delicious Purchase! INR 725.00 spent on your IDFC FIRST \
                       BANK Credit Card ending XX3620 at TRUFFLES HOSPITALITY PVT on 05 AUG \
                       2025. Available Limit: INR 38666.18 .",
                amount_minor: 72500,
                direction: "debit",
                merchant: Some("TRUFFLES HOSPITALITY PVT"),
                last4: Some("3620"),
                date: Some((2025, 8, 5)),
                balance_minor: Some(3866618),
            },
            Case {
                bank: "IDFC FIRST Bank",
                body: "Dear Customer, Payment of Rs. 6,283.37 was received on your FIRST \
                       Millennia Credit Card ending with XX3620 on 29 Nov 2025.",
                amount_minor: 628337,
                direction: "credit",
                merchant: Some("FIRST Millennia"),
                last4: Some("3620"),
                date: Some((2025, 11, 29)),
                balance_minor: None,
            },
            Case {
                bank: "Axis Bank",
                body: "17-08-2025 Dear Aditya Rawal, Thank you for using your credit card no. \
                       XX3825 for INR 379 at AIRTEL PAYM on 17-08-2025 00:41:37 IST.",
                amount_minor: 37900,
                direction: "debit",
                merchant: Some("AIRTEL PAYM"),
                last4: Some("3825"),
                date: Some((2025, 8, 17)),
                balance_minor: None,
            },
            Case {
                bank: "Yes Bank",
                body: "Dear Cardmember, INR 2441.98 has been spent on your YES BANK Credit Card \
                       ending with 2982 at UPI_RELIANCE BP MOBILI on 26-10-2025 at 01:47:41 pm. \
                       Avl Bal INR 95138.98.",
                amount_minor: 244198,
                direction: "debit",
                merchant: Some("UPI_RELIANCE BP MOBILI"),
                last4: Some("2982"),
                date: Some((2025, 10, 26)),
                balance_minor: Some(9513898),
            },
            Case {
                bank: "Jupiter",
                body: "Hey, Aditya Your UPI payment was successful You paid ₹543 Paid to \
                       HONGKONG NOODLES Vyapar.169687998887@hdfcbank Date Jan 01, 2026 From \
                       Aditya 8127696200@jupiteraxis Transaction ID 1321767280821724605",
                amount_minor: 54300,
                direction: "debit",
                merchant: Some("HONGKONG NOODLES"),
                last4: None,
                date: Some((2026, 1, 1)),
                balance_minor: None,
            },
        ];

        let pool = dummy_pool();
        for c in cases {
            let got = BankTemplateLayer
                .extract(&pool, c.bank, c.body)
                .await
                .unwrap_or_else(|| panic!("{}: no template matched a real body", c.bank));

            assert_eq!(got.amount_minor, Some(c.amount_minor), "{} amount", c.bank);
            assert_eq!(
                got.direction.as_deref(),
                Some(c.direction),
                "{} direction",
                c.bank
            );
            assert_eq!(
                got.merchant_raw.as_deref(),
                c.merchant,
                "{} merchant",
                c.bank
            );
            assert_eq!(
                got.masked_identifier.as_deref(),
                c.last4,
                "{} last4",
                c.bank
            );
            assert_eq!(
                got.balance_after, c.balance_minor,
                "{} balance_after",
                c.bank
            );
            if let Some((y, m, d)) = c.date {
                assert_eq!(
                    got.event_time,
                    Some(ymd_ts(y, m, d)),
                    "{} event_time",
                    c.bank
                );
            }
            assert!(
                got.is_valid(),
                "{}: template matched but the result fails is_valid(), so the ladder \
                 would discard it and fall through to Layer 3",
                c.bank
            );
        }
    }

    #[tokio::test]
    async fn test_mandate_pattern_routes_to_mandate_extractor_only() {
        let body = "Dear Cardholder, Thank you for registering for a recurring e-Mandate at \
                    merchant platform using your SBI Credit Card. Your e-Mandate set at merchant \
                    with SBI Credit Card ending 7603 has been registered. Merchant: ScribdInc \
                    Description: PremiumMonthlyMembership e-Mandate Limit Amount (INR): 1000.00 \
                    Frequency: monthly Start date: 21/04/2026 SiHub ID: YPCojLhIn2";

        let m = crate::extraction::mandate_extractor::bank_mandate_template("SBI Card", body)
            .expect("SBI Card mandate template must match a real registration body");
        assert_eq!(m.merchant.as_deref(), Some("ScribdInc"));
        assert_eq!(m.cadence.as_deref(), Some("monthly"));
        assert_eq!(m.max_limit_amount, Some(100_000));
        assert_eq!(m.external_mandate_id.as_deref(), Some("YPCojLhIn2"));
        assert_eq!(m.masked_identifier.as_deref(), Some("7603"));
        assert_eq!(m.instrument_type.as_deref(), Some("credit_card"));

        let as_txn = BankTemplateLayer
            .extract(&dummy_pool(), "SBI Card", body)
            .await;
        assert!(
            as_txn.is_none_or(|r| r.amount_minor != Some(100_000)),
            "a mandate limit must not be booked as a transaction amount"
        );
    }

    #[test]
    fn test_txn_types_map_to_valid_instrument_enum() {
        const SCHEMA_INSTRUMENT_TYPES: &[&str] = &[
            "credit_card",
            "debit_card",
            "bank_account",
            "UPI",
            "NEFT",
            "RTGS",
            "SWIFT",
            "upi_vpa",
            "wallet",
            "POS",
            "ATM",
            "cheque",
        ];
        for t in TEMPLATE_TXN_TYPES {
            if let Some(mapped) = instrument_type_for_txn_type(t) {
                assert!(
                    SCHEMA_INSTRUMENT_TYPES.contains(&mapped.as_str()),
                    "txn_type {t:?} maps to instrument_type {mapped:?}, which the \
                     instruments.type CHECK constraint would reject"
                );
            }
        }
        assert_eq!(
            instrument_type_for_txn_type("mandate"),
            None,
            "a mandate is an authorisation, not an instrument"
        );
    }

    #[test]
    fn test_every_registry_bank_has_a_template() {
        let registry: crate::ingestion::verified_senders::VerifiedSenderRegistry =
            serde_json::from_str(include_str!("../ingestion/verified_senders_registry.json"))
                .expect("registry must parse");

        let templates = bank_templates();
        for sender in &registry.senders {
            if sender.classification == "noise" {
                continue;
            }
            assert!(
                templates.contains_key(&sender.bank_name),
                "registry bank {:?} (domain {}) has no bank_templates/*.json -- \
                 it would silently skip Layer 2 entirely",
                sender.bank_name,
                sender.domain
            );
        }

        let registry_banks: std::collections::HashSet<&str> = registry
            .senders
            .iter()
            .map(|s| s.bank_name.as_str())
            .collect();
        for bank in templates.keys() {
            assert!(
                registry_banks.contains(bank.as_str()),
                "bank_templates has {bank:?} but no registry sender maps to it -- \
                 stale file, or the registry renamed the bank"
            );
        }
    }

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

    #[tokio::test]
    async fn test_sbi_intro_clause_boilerplate_does_not_win_over_real_merchant() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "Dear Cardholder,\nThis is to inform you that, Rs.245.43 spent on your SBI Credit Card ending 7603 at DREAMPLUGTECHNOLOGI on 01/07/26. Trxn. not done by you? Report at https://sbicard.com/Dispute. If you have not authorized this transaction please contact the SBI Card Helpline.";
        let result = layer.extract(&pool, "SBI Card", body).await.unwrap();
        assert_eq!(result.merchant_raw, Some("DREAMPLUGTECHNOLOGI".to_string()));
    }

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

    #[tokio::test]
    async fn test_orchestrator_stops_at_first_valid_layer() {
        let pool = setup_db_with_rule("active".to_string()).await;
        let body = "Your amount is 1500 INR at Amazon debit time 1700000000";

        let mut layer6_timed_out = false;
        let result = run_extraction_ladder(
            &pool,
            "Chase",
            body,
            None,
            false,
            None,
            &mut layer6_timed_out,
            None,
        )
        .await
        .unwrap();

        assert!(result.is_some());
        assert_eq!(result.unwrap().extraction_method, "learned_fields");
    }

    #[tokio::test]
    async fn test_learned_merchant_rule_overrides_a_later_layers_merchant() {
        let body = "Rs 500.00 debited at RAZ*SWIGGY on 25-May-23 towards purchase";
        let pool = dummy_migrated_pool().await;

        let mut timed_out = false;
        let before = run_extraction_ladder(
            &pool,
            "HDFC Bank",
            body,
            None,
            false,
            None,
            &mut timed_out,
            None,
        )
        .await
        .unwrap()
        .expect("fixture must extract");
        let baseline_merchant = before.merchant_raw.clone();

        let conn = pool.get().await.unwrap();
        let body_owned = body.to_string();
        conn.interact(move |c| {
            let template_hash = compute_template_hash(&body_owned);
            let pattern =
                crate::extraction::rule_synthesis::synthesize_span_regex(&body_owned, "RAZ*SWIGGY")
                    .expect("must synthesize a pattern");
            seed_rule(
                c,
                "HDFC Bank",
                "merchant",
                &body_owned,
                serde_json::json!({ "regex": pattern, "capture_group": 1 }),
                "active",
            );
            let _ = template_hash;
        })
        .await
        .unwrap();
        drop(conn);

        let after = run_extraction_ladder(
            &pool,
            "HDFC Bank",
            body,
            None,
            false,
            None,
            &mut timed_out,
            None,
        )
        .await
        .unwrap()
        .expect("fixture must still extract");

        assert_eq!(
            after.merchant_raw.as_deref(),
            Some("RAZ*SWIGGY"),
            "the learned rule must decide the merchant (baseline was {baseline_merchant:?})"
        );
    }

    #[tokio::test]
    async fn test_learned_merchant_rule_does_not_leak_to_other_email_shapes() {
        let taught_body = "Rs 500.00 debited at RAZ*SWIGGY on 25-May-23 towards purchase";
        let other_body =
            "INR 250.00 spent using your card at BIG BAZAAR on 26-May-23 towards purchase";
        let pool = dummy_migrated_pool().await;

        let conn = pool.get().await.unwrap();
        let taught = taught_body.to_string();
        conn.interact(move |c| {
            let template_hash = compute_template_hash(&taught);
            let pattern =
                crate::extraction::rule_synthesis::synthesize_span_regex(&taught, "RAZ*SWIGGY")
                    .unwrap();
            seed_rule(
                c,
                "HDFC Bank",
                "merchant",
                &taught,
                serde_json::json!({ "regex": pattern, "capture_group": 1 }),
                "active",
            );
            let _ = template_hash;
        })
        .await
        .unwrap();
        drop(conn);

        let mut timed_out = false;
        let other = run_extraction_ladder(
            &pool,
            "HDFC Bank",
            other_body,
            None,
            false,
            None,
            &mut timed_out,
            None,
        )
        .await
        .unwrap()
        .expect("the unrelated email must still extract");

        assert_ne!(
            other.merchant_raw.as_deref(),
            Some("RAZ*SWIGGY"),
            "a rule taught on one email shape must not rewrite a different one"
        );
    }

    #[tokio::test]
    async fn test_ensemble_lite_amount_disagreement_downgrades_confidence() {
        let body = "Txn ID 999900 INR for your purchase. Rs 500.00 debited at Amazon on 25-May-23";
        let pool = dummy_migrated_pool().await;
        let conn = pool.get().await.unwrap();
        let body_owned = body.to_string();
        conn.interact(move |c| {
            let template_hash = compute_template_hash(&body_owned);
            for (field, regex) in [
                ("amount", r"Txn ID (\d+)"),
                ("merchant", "at ([A-Za-z]+)"),
                ("currency", "([A-Z]{3})"),
                ("direction", "(debited)"),
                ("event_time", r"on (\d{2}-[A-Za-z]{3}-\d{2})"),
            ] {
                seed_rule(
                    c,
                    "WrongRuleBank",
                    field,
                    &body_owned,
                    serde_json::json!({"regex": regex, "capture_group": 1}),
                    "active",
                );
            }
            let _ = template_hash;
        })
        .await
        .unwrap();

        let mut layer6_timed_out = false;
        let result = run_extraction_ladder(
            &pool,
            "WrongRuleBank",
            body,
            None,
            false,
            None,
            &mut layer6_timed_out,
            None,
        )
        .await
        .unwrap()
        .expect("the (wrong) learned rule is schema-valid and must still be returned");

        assert_eq!(result.extraction_method, "learned_fields");
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

    #[tokio::test]
    async fn test_orchestrator_fails_if_all_layers_empty() {
        use tracing_subscriber::layer::SubscriberExt;
        struct NoopLayer;
        impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for NoopLayer {}
        let _guard =
            tracing::subscriber::set_default(tracing_subscriber::registry().with(NoopLayer));

        let pool = dummy_pool();
        let mut layer6_timed_out = false;
        let res = run_extraction_ladder(
            &pool,
            "Chase",
            "unparseable body",
            None,
            false,
            None,
            &mut layer6_timed_out,
            None,
        )
        .await
        .unwrap();
        assert!(res.is_none());
    }

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
        let res = run_extraction_ladder(
            &pool,
            "Chase",
            "unparseable body",
            None,
            false,
            None,
            &mut layer6_timed_out,
            None,
        )
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
        assert_eq!(compute_template_hash(b1), compute_template_hash(b2));
        assert_eq!(
            compute_template_hash(b1),
            "89a6278bc760568ecab7942236a60ca7d96b7ebcf19b98302c4465d2d6485c0b",
            "template hash changed -- every persisted template_hash is now orphaned"
        );
    }

    fn seed_rule(
        conn: &rusqlite::Connection,
        bank: &str,
        field: &str,
        source_body: &str,
        payload: serde_json::Value,
        status: &str,
    ) {
        let now = chrono::Utc::now().naive_utc();
        crate::db::field_rules::upsert_variant(
            conn,
            &crate::db::field_rules::FieldRuleVariant {
                id: uuid::Uuid::new_v4().to_string(),
                bank_name: bank.to_string(),
                field_name: field.to_string(),
                source_type: "email".to_string(),
                template_hash: compute_template_hash(source_body),
                rule_payload_json: payload,
                status: status.to_string(),
                success_count: 5,
                failure_count: 0,
                confidence: 1.0,
                authored_by: "deterministic".to_string(),
                learned_from: "user_edit".to_string(),
                created_at: Some(now),
                updated_at: Some(now),
            },
            None,
        )
        .unwrap();
    }

    const LEARNED_RULE_BODY: &str = "Your amount is 1500 INR at Amazon debit time 1700000000";

    async fn setup_db_with_rule(status: String) -> Pool {
        let pool = dummy_migrated_pool().await;
        let conn = pool.get().await.unwrap();
        conn.interact(move |c| {
            for (field, regex) in [
                ("amount", "amount is ([0-9]+) INR"),
                ("merchant", "at ([A-Za-z]+)"),
                ("currency", "([A-Z]{3})"),
                ("direction", "(debit)"),
                ("event_time", "time ([0-9]+)"),
            ] {
                seed_rule(
                    c,
                    "Chase",
                    field,
                    LEARNED_RULE_BODY,
                    serde_json::json!({"regex": regex, "capture_group": 1}),
                    &status,
                );
            }
        })
        .await
        .unwrap();
        pool
    }

    #[tokio::test]
    async fn a_merchant_only_rule_overrides_the_winning_layer() {
        let pool = dummy_migrated_pool().await;
        let body = "Rs 500 spent at RAZ*SWIGGY LIMITE on 01/07/26 via card 1234";
        {
            let conn = pool.get().await.unwrap();
            let b = body.to_string();
            conn.interact(move |c| {
                seed_rule(
                    c,
                    "HDFC Bank",
                    "merchant",
                    &b,
                    serde_json::json!({"regex": r"at\s+(.{1,80}?)\s+on", "capture_group": 1}),
                    "active",
                )
            })
            .await
            .unwrap();
        }

        let mut result = ExtractionResult {
            merchant_raw: Some("WRONG".to_string()),
            amount_minor: Some(50000),
            currency: Some("INR".to_string()),
            direction: Some("debit".to_string()),
            event_time: Some(1_780_000_000),
            ..Default::default()
        };
        let fired = apply_learned_fields(&pool, "HDFC Bank", body, "email", &mut result).await;

        assert!(fired);
        assert_eq!(result.merchant_raw.as_deref(), Some("RAZ*SWIGGY LIMITE"));
        assert_eq!(
            result.amount_minor,
            Some(50000),
            "untaught fields must be left alone"
        );
    }

    #[tokio::test]
    async fn an_amount_rule_parses_into_minor_units() {
        let pool = dummy_migrated_pool().await;
        let body = "INR 1,020.00 debited from your account on 01/07/26";
        {
            let conn = pool.get().await.unwrap();
            let b = body.to_string();
            conn.interact(move |c| {
                seed_rule(
                    c,
                    "HDFC Bank",
                    "amount",
                    &b,
                    serde_json::json!({"regex": r"INR\s+([\d,.]+)\s", "capture_group": 1}),
                    "active",
                )
            })
            .await
            .unwrap();
        }
        let mut result = ExtractionResult::default();
        apply_learned_fields(&pool, "HDFC Bank", body, "email", &mut result).await;
        assert_eq!(result.amount_minor, Some(102000));
    }

    #[tokio::test]
    async fn an_override_applies_only_to_its_own_template() {
        let pool = dummy_migrated_pool().await;
        let taught = "Rs 500 credited to your account on 01/07/26";
        let other = "Your statement for June is ready. Total due Rs 900.";
        {
            let conn = pool.get().await.unwrap();
            let b = taught.to_string();
            conn.interact(move |c| {
                seed_rule(
                    c,
                    "HDFC Bank",
                    "direction",
                    &b,
                    serde_json::json!({"override_value": "credit"}),
                    "active",
                )
            })
            .await
            .unwrap();
        }

        let mut on_template = ExtractionResult {
            direction: Some("debit".to_string()),
            ..Default::default()
        };
        apply_learned_fields(&pool, "HDFC Bank", taught, "email", &mut on_template).await;
        assert_eq!(on_template.direction.as_deref(), Some("credit"));

        let mut off_template = ExtractionResult {
            direction: Some("debit".to_string()),
            ..Default::default()
        };
        apply_learned_fields(&pool, "HDFC Bank", other, "email", &mut off_template).await;
        assert_eq!(
            off_template.direction.as_deref(),
            Some("debit"),
            "an override must never leak to a different template shape"
        );
    }

    #[tokio::test]
    async fn learned_rules_never_cross_banks() {
        let pool = dummy_migrated_pool().await;
        let body = "Rs 500 spent at SWIGGY on 01/07/26";
        {
            let conn = pool.get().await.unwrap();
            let b = body.to_string();
            conn.interact(move |c| {
                seed_rule(
                    c,
                    "HDFC Bank",
                    "merchant",
                    &b,
                    serde_json::json!({"regex": r"at\s+(.{1,80}?)\s+on", "capture_group": 1}),
                    "active",
                )
            })
            .await
            .unwrap();
        }
        let mut result = ExtractionResult::default();
        let fired = apply_learned_fields(&pool, "ICICI Bank", body, "email", &mut result).await;
        assert!(!fired);
        assert!(result.merchant_raw.is_none());
    }

    #[tokio::test]
    async fn a_rule_that_does_not_match_this_body_is_simply_skipped() {
        let pool = dummy_migrated_pool().await;
        let taught = "Rs 500 spent at SWIGGY on 01/07/26";
        {
            let conn = pool.get().await.unwrap();
            let b = taught.to_string();
            conn.interact(move |c| {
                seed_rule(
                    c,
                    "HDFC Bank",
                    "merchant",
                    &b,
                    serde_json::json!({"regex": r"at\s+(.{1,80}?)\s+on", "capture_group": 1}),
                    "active",
                )
            })
            .await
            .unwrap();
        }
        let mut result = ExtractionResult::default();
        let fired = apply_learned_fields(
            &pool,
            "HDFC Bank",
            "A totally different message shape entirely.",
            "email",
            &mut result,
        )
        .await;
        assert!(!fired, "coexistence across templates depends on this");
    }

    #[tokio::test]
    async fn the_layer_returns_none_without_a_complete_result() {
        let pool = dummy_migrated_pool().await;
        let body = "Rs 500 spent at SWIGGY on 01/07/26";
        {
            let conn = pool.get().await.unwrap();
            let b = body.to_string();
            conn.interact(move |c| {
                seed_rule(
                    c,
                    "HDFC Bank",
                    "merchant",
                    &b,
                    serde_json::json!({"regex": r"at\s+(.{1,80}?)\s+on", "capture_group": 1}),
                    "active",
                )
            })
            .await
            .unwrap();
        }
        assert!(LearnedFieldLayer
            .extract(&pool, "HDFC Bank", body)
            .await
            .is_none());
    }

    #[tokio::test]
    async fn drift_is_not_declared_for_an_unknown_template() {
        let pool = dummy_migrated_pool().await;
        let conn = pool.get().await.unwrap();
        let drift = conn
            .interact(|c| detect_pattern_drift(c, "HDFC Bank", "a body never seen before", &None))
            .await
            .unwrap()
            .unwrap();
        assert!(!drift.drift_detected);
    }

    #[tokio::test]
    async fn drift_is_declared_when_a_known_template_stops_extracting() {
        let pool = dummy_migrated_pool().await;
        let body = "Rs 500 spent at SWIGGY on 01/07/26";
        let conn = pool.get().await.unwrap();
        let b = body.to_string();
        let drift = conn
            .interact(move |c| {
                seed_rule(
                    c,
                    "HDFC Bank",
                    "merchant",
                    &b,
                    serde_json::json!({"regex": r"at\s+(.{1,80}?)\s+on", "capture_group": 1}),
                    "active",
                );
                detect_pattern_drift(c, "HDFC Bank", &b, &None)
            })
            .await
            .unwrap()
            .unwrap();
        assert!(
            drift.drift_detected,
            "rules exist for this shape yet nothing extracted"
        );
    }

    #[tokio::test]
    async fn a_successful_extraction_is_never_drift() {
        let pool = dummy_migrated_pool().await;
        let body = "Rs 500 spent at SWIGGY on 01/07/26";
        let conn = pool.get().await.unwrap();
        let b = body.to_string();
        let drift = conn
            .interact(move |c| {
                seed_rule(
                    c,
                    "HDFC Bank",
                    "merchant",
                    &b,
                    serde_json::json!({"regex": r"at\s+(.{1,80}?)\s+on", "capture_group": 1}),
                    "active",
                );
                detect_pattern_drift(c, "HDFC Bank", &b, &Some(ExtractionResult::default()))
            })
            .await
            .unwrap()
            .unwrap();
        assert!(!drift.drift_detected);
    }

    #[tokio::test]
    async fn drift_does_not_see_a_statement_rule_as_a_known_email_template() {
        let pool = dummy_migrated_pool().await;
        let body = "Rs 500 spent at SWIGGY on 01/07/26";
        let conn = pool.get().await.unwrap();
        let b = body.to_string();
        let drift = conn
            .interact(move |c| {
                let now = chrono::Utc::now().naive_utc();
                crate::db::field_rules::upsert_variant(
                    c,
                    &crate::db::field_rules::FieldRuleVariant {
                        id: "pdf_rule".to_string(),
                        bank_name: "HDFC Bank".to_string(),
                        field_name: "merchant".to_string(),
                        source_type: "statement_pdf".to_string(),
                        template_hash: compute_template_hash(&b),
                        rule_payload_json: serde_json::json!({
                            "regex": "(.+)", "capture_group": 1
                        }),
                        status: "active".to_string(),
                        success_count: 5,
                        failure_count: 0,
                        confidence: 1.0,
                        authored_by: "deterministic".to_string(),
                        learned_from: "user_edit".to_string(),
                        created_at: Some(now),
                        updated_at: Some(now),
                    },
                    None,
                )
                .unwrap();
                detect_pattern_drift(c, "HDFC Bank", &b, &None)
            })
            .await
            .unwrap()
            .unwrap();
        assert!(!drift.drift_detected);
    }

    #[tokio::test]
    async fn test_learned_rule_applied_when_active() {
        let pool = setup_db_with_rule("active".to_string()).await;
        let layer = LearnedFieldLayer;
        let body = "Your amount is 1500 INR at Amazon debit time 1700000000";

        let result = layer.extract(&pool, "Chase", body).await;

        assert!(result.is_some());
        let res = result.unwrap();
        assert_eq!(res.amount_minor, Some(150000));
        assert_eq!(res.merchant_raw, Some("Amazon".to_string()));
        assert_eq!(res.currency, Some("INR".to_string()));
        assert_eq!(res.direction, Some("debit".to_string()));
        assert_eq!(res.extraction_method, "learned_fields");
    }

    #[tokio::test]
    async fn test_learned_rule_matches_across_different_templates() {
        let old_body = "Your amount is 1500 INR at Amazon debit time 1700000000";
        let pool = setup_db_with_rule("active".to_string()).await;

        let new_body =
            "Reminder: your amount is 1500 INR at Amazon debit time 1700000000 -- thank you.";
        assert_ne!(
            compute_template_hash(old_body),
            compute_template_hash(new_body),
            "the two bodies must hash differently to actually exercise cross-template matching"
        );

        let layer = LearnedFieldLayer;
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
        let layer = LearnedFieldLayer;
        let body = "Your amount is 1500 INR at Amazon debit time 1700000000";

        let result = layer.extract(&pool, "Chase", body).await;

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_pending_rule_not_auto_applied() {
        let pool = setup_db_with_rule("pending".to_string()).await;
        let layer = LearnedFieldLayer;
        let body = "Your amount is 1500 INR at Amazon debit time 1700000000";

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
        assert_eq!(result_4.event_time, Some(ymd_ts(2023, 5, 25)));
    }

    #[tokio::test]
    async fn test_bank_template_invalid_date_no_fallback_leaves_event_time_none() {
        let pool = dummy_pool();
        let layer = BankTemplateLayer;
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

    #[test]
    fn test_date_parsers_return_none_not_fake_sentinel_on_failure() {
        assert_eq!(parse_date_generic("not a date"), None);
        assert_eq!(parse_date_generic("35-May-23"), None);
        assert_eq!(parse_date_generic("32/13/26"), None);
    }

    #[test]
    fn test_real_bank_date_formats_parse() {
        for (input, y, m, d) in [
            ("23-12-25", 2025, 12, 23),
            ("10/01/26", 2026, 1, 10),
            ("08-JAN-26", 2026, 1, 8),
            ("30-JUL-2025", 2025, 7, 30),
            ("05 AUG 2025", 2025, 8, 5),
            ("29 Nov 2025", 2025, 11, 29),
            ("17-08-2025", 2025, 8, 17),
            ("07 Jan, 2026", 2026, 1, 7),
            ("Jan 08, 2026", 2026, 1, 8),
            ("Mon, Dec 01, 2025", 2025, 12, 1),
            ("29-Dec-25", 2025, 12, 29),
        ] {
            let parsed = parse_date_generic(input)
                .unwrap_or_else(|| panic!("real bank date {input:?} must parse"));
            assert_eq!(
                parsed.timestamp,
                ymd_ts(y, m, d),
                "{input:?} parsed to the wrong day"
            );
        }
    }

    fn ymd_ts(year: i32, month: u32, day: u32) -> i64 {
        chrono::NaiveDate::from_ymd_opt(year, month, day)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp()
    }

    #[test]
    fn test_parse_date_generic_ambiguous_flag() {
        let unambiguous_numeric = parse_date_generic("25/05/2023").unwrap();
        assert!(!unambiguous_numeric.ambiguous);

        let month_name = parse_date_generic("05-Aug-2026").unwrap();
        assert!(!month_name.ambiguous);

        let ambiguous = parse_date_generic("02-07-2026").unwrap();
        assert!(ambiguous.ambiguous);
        assert_eq!(ambiguous.timestamp, ymd_ts(2026, 7, 2));

        let noop_swap = parse_date_generic("05-05-2026").unwrap();
        assert!(!noop_swap.ambiguous);
    }

    #[test]
    fn test_apply_date_cross_check_noop_when_not_flagged_ambiguous() {
        let original_ts = ymd_ts(2026, 8, 5);
        let mut obs = ExtractionResult {
            event_time: Some(original_ts),
            event_time_ambiguous: false,
            ..Default::default()
        };
        let anchor = Some(ymd_ts(2026, 5, 5));

        apply_date_cross_check(&mut obs, anchor);

        assert_eq!(obs.event_time, Some(original_ts));
        assert_eq!(obs.date_cross_check_flag, None);
    }

    #[test]
    fn test_apply_date_cross_check_decisive_swap() {
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

    #[test]
    fn test_apply_date_cross_check_weak_signal_untouched() {
        let original_ts = ymd_ts(2026, 7, 2);
        let mut obs = ExtractionResult {
            event_time: Some(original_ts),
            event_time_ambiguous: true,
            ..Default::default()
        };
        let anchor = Some(ymd_ts(2026, 7, 3));

        apply_date_cross_check(&mut obs, anchor);

        assert_eq!(obs.event_time, Some(original_ts));
        assert_eq!(obs.date_cross_check_flag, None);
    }

    #[test]
    fn test_apply_date_cross_check_both_implausible_flags_for_review() {
        let original_ts = ymd_ts(2026, 7, 2);
        let mut obs = ExtractionResult {
            event_time: Some(original_ts),
            event_time_ambiguous: true,
            confidence_score: Some(0.6),
            ..Default::default()
        };
        let anchor = Some(ymd_ts(2026, 10, 2));

        apply_date_cross_check(&mut obs, anchor);

        assert_eq!(obs.event_time, Some(original_ts));
        assert_eq!(
            obs.date_cross_check_flag,
            Some("anchor_mismatch_needs_review".to_string())
        );
        assert!(obs.confidence_score.unwrap() <= CROSS_CHECK_DISAGREEMENT_CONFIDENCE);
    }

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

    #[tokio::test]
    async fn test_generic_confidence_varies_by_field_strength() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;

        let strong_body =
            "You have paid Rs 1,500.50 paid to Zomato via UPI on 25/05/2023. Ref: 123456789012.";
        let strong = layer.extract(&pool, "Any Bank", strong_body).await.unwrap();
        assert_eq!(strong.confidence_score, Some(LAYER3_MAX_CONFIDENCE));

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

    #[tokio::test]
    async fn test_generic_merchant_heuristic() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "Rs 1,500.50 paid to Zomato via UPI on 25/05/2023.";
        let result = layer.extract(&pool, "Any Bank", body).await.unwrap();
        assert_eq!(result.merchant_raw, Some("Zomato".to_string()));
    }

    #[tokio::test]
    async fn test_generic_merchant_heuristic_towards() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "Rs 250.00 paid towards Swiggy via UPI on 25/05/2023.";
        let result = layer.extract(&pool, "Any Bank", body).await.unwrap();
        assert_eq!(result.merchant_raw, Some("Swiggy".to_string()));
    }

    #[tokio::test]
    async fn test_generic_merchant_heuristic_info_colon() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "Rs 99.00 debited on 25/05/2023. Info: Starbucks Coffee";
        let result = layer.extract(&pool, "Any Bank", body).await.unwrap();
        assert_eq!(result.merchant_raw, Some("Starbucks Coffee".to_string()));
    }

    #[tokio::test]
    async fn test_generic_merchant_heuristic_asterisk_descriptor() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "Rs. 2590.00 has been debited from your HDFC Bank Credit Card ending 0364 towards RAZ*SWIGGY on 24 May, 2026 at 19:34:18 .";
        let result = layer.extract(&pool, "HDFC Bank", body).await.unwrap();
        assert_eq!(result.merchant_raw, Some("RAZ*SWIGGY".to_string()));
        assert_eq!(result.amount_minor, Some(259000));
    }

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

    #[tokio::test]
    async fn test_generic_merchant_skips_self_referential_account() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "₹17000.0 was credited to your account\nYou've received ₹17000.0 in Federal Bank Savings Account ending with 1527.\nPayment from:                                ADITYA RAWAL\nDate                                Jun 30, 2026";
        let result = layer.extract(&pool, "Jupiter", body).await.unwrap();
        assert_eq!(result.merchant_raw, Some("ADITYA RAWAL".to_string()));
        assert_eq!(result.amount_minor, Some(1700000));
    }

    #[tokio::test]
    async fn test_generic_regex_underscore_merchant_and_disclaimer_footer() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "Dear Customer, Greetings from YES BANK. INR 91.00 has been spent on your YES BANK Credit Card ending with 2982 at UPI_SRI SAI FRUITS AND on 10-07-2026 at 08:55:35 pm. Avl Bal INR 82434.42. In case, this transaction was not initiated by you, please block your card immediately by calling our 24x7 customer care or visiting the nearest branch.";
        let result = layer.extract(&pool, "Yes Bank", body).await.unwrap();
        assert_eq!(
            result.merchant_raw,
            Some("UPI_SRI SAI FRUITS AND".to_string())
        );
        assert_eq!(result.direction, Some("debit".to_string()));
        assert_eq!(result.amount_minor, Some(9100));
    }

    #[tokio::test]
    async fn test_generic_merchant_rejects_stopword_only_disclaimer_capture() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body =
            "INR 250.00 debited. To block your card, SMS BLOCK to 9876543210 or call our helpline.";
        let result = layer.extract(&pool, "Yes Bank", body).await;
        if let Some(r) = result {
            assert_ne!(r.merchant_raw, Some("block your".to_string()));
        }
    }

    #[tokio::test]
    async fn test_generic_amount_recognizes_spelled_out_iso_currency_code() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "A transaction of USD 1.00 on your YES BANK Credit Card ending 2982 on 20-05-2026 at 11:57:54 pm at OPENAI is declined because International Ecom/online transactions are disabled on your card.";
        let result = layer.extract(&pool, "Yes Bank", body).await.unwrap();
        assert_eq!(result.amount_minor, Some(100));
        assert_eq!(result.currency, Some("USD".to_string()));
    }

    #[tokio::test]
    async fn test_generic_merchant_terminates_before_declined_prose() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "A transaction of USD 1.00 on your YES BANK Credit Card ending 2982 on 20-05-2026 at 11:57:54 pm at OPENAI is declined because International Ecom/online transactions are disabled on your card. To enable,please visit iris by YES BANK app.";
        let result = layer.extract(&pool, "Yes Bank", body).await.unwrap();
        assert_eq!(result.merchant_raw, Some("OPENAI".to_string()));
    }

    #[test]
    fn test_generic_date_fallback_to_internal_date() {
        use crate::ingestion::message_processor::MessageProcessor;

        assert_eq!(MessageProcessor::internal_date_fallback(&None), None);

        let internal_date = Some("1700000000000".to_string());
        assert_eq!(
            MessageProcessor::internal_date_fallback(&internal_date),
            Some(1_700_000_000)
        );

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

    #[tokio::test]
    async fn test_nlp_first_valid_merchant_not_overwritten_by_later_disclaimer() {
        let pool = dummy_pool();
        let layer = NlpLayer;
        let body = "Rs 500.00 debited from HDFC Bank A/c ending 1234 at Amazon on 25-May-23 Bal Rs 1000.00. To block your card immediately, call our helpline.";
        let result = layer.extract(&pool, "HDFC Bank", body).await.unwrap();

        assert_eq!(result.merchant_raw, Some("Amazon".to_string()));
    }

    #[test]
    fn test_instrument_signals_credit_card_last4() {
        let body =
            "Rs 1500.00 spent on your HDFC Bank CREDIT Card ending 1234 at Amazon on 25-May-23.";
        let signals = extract_instrument_signals("HDFC Bank", body);
        assert_eq!(signals.masked_identifier, Some("1234".to_string()));
        assert_eq!(signals.instrument_type, Some("credit_card".to_string()));
        assert_eq!(signals.issuer_name, Some("HDFC Bank".to_string()));
    }

    #[test]
    fn test_instrument_signals_bank_account_suffix() {
        let body = "Rs 500.00 debited from HDFC Bank A/c ending 5678 at Amazon on 25-May-23.";
        let signals = extract_instrument_signals("HDFC Bank", body);
        assert_eq!(signals.masked_identifier, Some("5678".to_string()));
        assert_eq!(signals.instrument_type, Some("bank_account".to_string()));
        assert_eq!(signals.issuer_name, Some("HDFC Bank".to_string()));
    }

    #[test]
    fn test_instrument_signals_edge_cases() {
        let s1 = extract_instrument_signals("Bank", "card ending XXXX1234");
        assert_eq!(s1.masked_identifier, Some("1234".to_string()));

        let s2 = extract_instrument_signals("Bank", "card ending XXXXXX1234");
        assert_eq!(s2.masked_identifier, Some("1234".to_string()));

        let s3 = extract_instrument_signals("Bank", "card ending 1234");
        assert_eq!(s3.masked_identifier, Some("1234".to_string()));

        let s4 = extract_instrument_signals("Bank", "card ending XXXX34");
        assert_eq!(s4.masked_identifier, Some("34".to_string()));

        let s5 = extract_instrument_signals("Bank", "account XXXX 1234");
        assert_eq!(s5.masked_identifier, Some("1234".to_string()));

        let s6 = extract_instrument_signals("Bank", "card ending XXXX XXXX 1234");
        assert_eq!(s6.masked_identifier, Some("1234".to_string()));

        let s7 = extract_instrument_signals("Bank", "card ending **** **** **** 1234");
        assert_eq!(s7.masked_identifier, Some("1234".to_string()));

        let s8 = extract_instrument_signals("Bank", "account no. XX-1234");
        assert_eq!(s8.masked_identifier, Some("1234".to_string()));
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
        assert_eq!(signals.masked_identifier, Some("4691".to_string()));
        assert_eq!(signals.instrument_type, Some("bank_account".to_string()));
        assert_eq!(signals.upi_vpa, None);
    }

    #[test]
    fn test_instrument_signals_network_detected() {
        let body =
            "Rs 1500.00 spent on your Axis Visa Credit Card ending 9999 at Flipkart on 01-Jan-24.";
        let signals = extract_instrument_signals("Axis Bank", body);
        assert_eq!(signals.network, Some("Visa".to_string()));
        assert_eq!(signals.masked_identifier, Some("9999".to_string()));
    }

    #[test]
    fn test_instrument_signals_no_match_returns_only_issuer() {
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
        assert_eq!(
            signals.masked_identifier,
            Some("8127696200@jupiteraxis".to_string())
        );
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
        let body =
            "Rs 1500.00 spent on your HDFC Bank CREDIT Card ending 1234 at Amazon on 25-May-23.";
        let mut layer6_timed_out = false;
        let result = run_extraction_ladder(
            &pool,
            "HDFC Bank",
            body,
            None,
            false,
            None,
            &mut layer6_timed_out,
            None,
        )
        .await
        .unwrap();
        assert!(result.is_some());
        let obs = result.unwrap();
        assert_eq!(obs.amount_minor, Some(150000));
        assert_eq!(obs.masked_identifier, Some("1234".to_string()));
        assert_eq!(obs.instrument_type, Some("credit_card".to_string()));
        assert_eq!(obs.issuer_name, Some("HDFC Bank".to_string()));
    }

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
                 VALUES ('inst_1', 'credit_card', 'HDFC Bank', '1234', 'active')",
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
        assert_eq!(obs.masked_identifier, Some("1234".to_string()));
    }

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

    #[tokio::test]
    async fn test_layer5_no_match_returns_none() {
        let anchor = chrono::NaiveDate::from_ymd_opt(2023, 5, 25).unwrap();
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

    #[tokio::test]
    async fn test_layer5_no_anchor_date_returns_none() {
        let pool = setup_crossref_db(vec![]).await;
        let body = "Rs 1500.00 spent on your HDFC Bank credit card ending 1234.";
        let result = Layer5CrossrefLayer
            .extract(&pool, "HDFC Bank", body, None)
            .await;
        assert!(result.is_none());
    }

    async fn setup_drift_db(bank_name: &str, body_to_register: &str) -> (Pool, String) {
        let pool = dummy_migrated_pool().await;
        let template_hash = compute_template_hash(body_to_register);
        let registered_body = body_to_register.to_string();
        let bank_name_str = bank_name.to_string();

        let conn = pool.get().await.unwrap();
        conn.interact(move |c| {
            seed_rule(
                c,
                &bank_name_str,
                "amount",
                &registered_body,
                serde_json::json!({ "regex": r"Rs ([\d,]+) spent", "capture_group": 1 }),
                "active",
            );
        })
        .await
        .unwrap();

        (pool, template_hash)
    }

    #[tokio::test]
    async fn test_drift_detected_for_changed_hdfc_template() {
        let original_body =
            "Rs 1500 spent on HDFC Bank CREDIT Card ending 1234 at Amazon on 25-May-23.";
        let (_pool, registered_hash) = setup_drift_db("HDFC Bank", original_body).await;

        let changed_body =
            "HDFC Bank: Transaction of INR 1500 done at merchant Amazon on 25-May-2023. New format.";
        let changed_hash = compute_template_hash(changed_body);
        assert_ne!(
            registered_hash, changed_hash,
            "Changed body must produce a different template hash to simulate drift"
        );

        let conn = crate::db::test_helpers::setup_test_db_async().await;

        let drift_new_template =
            detect_pattern_drift(&conn, "HDFC Bank", changed_body, &None).unwrap();
        assert!(
            !drift_new_template.drift_detected,
            "A genuinely new (never-seen) template must NOT be flagged as drift; \
             got drift_detected = true"
        );
        assert_eq!(drift_new_template.template_hash, changed_hash);

        seed_rule(
            &conn,
            "HDFC Bank",
            "amount",
            original_body,
            serde_json::json!({ "regex": r"Rs ([\d,]+) spent", "capture_group": 1 }),
            "active",
        );

        let drift_known_template =
            detect_pattern_drift(&conn, "HDFC Bank", original_body, &None).unwrap();
        assert!(
            drift_known_template.drift_detected,
            "Known template (active rules exist) + ladder returned None must be drift; \
             got drift_detected = false"
        );
        assert_eq!(drift_known_template.template_hash, registered_hash);

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

    #[tokio::test]
    async fn test_fx_transaction_extracted_correctly() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "Acct XX1234 debited USD 50.00 (INR 4150.50) on 25-May-23 at Netflix.";
        let result = layer.extract(&pool, "Any Bank", body).await;
        assert!(result.is_some());
    }

    #[tokio::test]
    async fn test_declined_transaction_rejected_or_flagged() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "Transaction of INR 500.00 at POS declined due to insufficient funds.";
        let result = layer.extract(&pool, "Any Bank", body).await;
        assert!(result.is_none() || result.unwrap().amount_minor.unwrap_or(0) > 0);
    }

    #[tokio::test]
    async fn test_multi_amount_format_picks_correct_amount() {
        let pool = dummy_pool();
        let layer = GenericRegexLayer;
        let body = "Spent INR 500.00. Available limit is INR 45,000.00.";
        let result = layer.extract(&pool, "Any Bank", body).await;
        if let Some(res) = result {
            assert_eq!(res.amount_minor, Some(50000));
        }
    }

    #[tokio::test]
    async fn test_icici_upi_on_credit_card_regex() {
        let pool = dummy_pool();
        let layer = BankTemplateLayer;
        let body = "Dear Customer, Credit Card XX1234 debited with INR 500.00 on 25-May-23. Info: UPI/1234567890/Amazon.";
        let result = layer.extract(&pool, "ICICI Bank", body).await;
        if let Some(res) = result {
            assert_eq!(res.amount_minor, Some(50000));
        }
    }

    #[test]
    fn test_detect_channel_hdfc_imps_self_transfer() {
        let body =
            "HDFC BANK\n\nDear Customer,\n\nGreetings from HDFC Bank!\n\n INR 1,04,721.00 has \
             been debited from your account ending xxxxxxxxxx4691 on 30-06-26 and credited to the \
             account ending xxxxxxxxxx1527 via IMPS.\n\nIMPS Reference No: 618139547133\nAvailable \
             Balance: INR 10,000.00";
        let obs = ExtractionResult::default();
        assert_eq!(
            detect_channel(&obs, body),
            Some("internal_transfer".to_string())
        );
    }

    #[test]
    fn test_detect_channel_upi_credit_card_requires_credit_card_instrument() {
        let body =
            "Rs 500.00 spent using your Credit Card ending 1234 at Amazon via UPI on 25-May-23.";

        let credit_card_obs = ExtractionResult {
            instrument_type: Some("credit_card".to_string()),
            ..Default::default()
        };
        assert_eq!(
            detect_channel(&credit_card_obs, body),
            Some("upi_credit_card".to_string())
        );

        let bank_account_obs = ExtractionResult::default();
        assert_eq!(
            detect_channel(&bank_account_obs, body),
            Some("upi".to_string())
        );
    }

    #[test]
    fn test_detect_channel_keyword_branches() {
        let obs = ExtractionResult::default();
        let cases: &[(&str, &str)] = &[
            ("Rs 500 debited towards NEFT transfer to XYZ.", "neft"),
            (
                "Rs 50000 transferred via RTGS to account ending 1234.",
                "rtgs",
            ),
            ("Rs 200 spent at POS terminal, Big Bazaar.", "pos"),
            ("Rs 2000 withdrawn from ATM at MG Road.", "atm"),
            ("Rs 150 loaded to your Paytm wallet.", "wallet"),
            ("Cheque no. 123456 cleared for Rs 10000.", "cheque"),
            ("Your NACH mandate for Rs 999 was debited.", "ecs_nach"),
            ("Your BNPL bill of Rs 500 is due.", "bnpl"),
            ("Your loan account has been disbursed Rs 100000.", "loan"),
        ];
        for (body, expected) in cases {
            assert_eq!(
                detect_channel(&obs, body),
                Some(expected.to_string()),
                "body: {body}"
            );
        }
    }

    #[test]
    fn test_detect_channel_emi_fallback_when_no_stronger_signal() {
        let obs = ExtractionResult {
            emi_total_installments: Some(6),
            ..Default::default()
        };
        let body = "Your purchase of Rs 6000 has been converted to EMI, 6 installments.";
        assert_eq!(detect_channel(&obs, body), Some("emi".to_string()));
    }

    #[test]
    fn test_detect_channel_none_when_no_signal_present() {
        let obs = ExtractionResult::default();
        let body = "Rs 500.00 credited to your account from a well-wisher.";
        assert_eq!(detect_channel(&obs, body), None);
    }

    #[tokio::test]
    async fn test_indusind_credit_card_txn_approved() {
        let pool = dummy_pool();
        let layer = BankTemplateLayer;
        let body = "The transaction on your IndusInd Bank Credit Card ending 7480 for INR \
            134.00 on 15-02-2026 09:25:43 pm at Swiggy Limited is Approved. Available Limit: \
            INR 49,866.00.";
        let result = layer
            .extract(&pool, "IndusInd Bank", body)
            .await
            .expect("credit_card_txn_approved pattern must match");
        assert_eq!(result.amount_minor, Some(13400));
        assert_eq!(result.merchant_raw, Some("Swiggy Limited".to_string()));
        assert_eq!(result.direction, Some("debit".to_string()));
    }

    #[tokio::test]
    async fn test_indusind_credit_card_bill_payment_thank_you() {
        let pool = dummy_pool();
        let layer = BankTemplateLayer;
        let body = "Dear Customer,\n\nThank you for your Payment of INR 134.00 towards your \
            IndusInd Bank Credit Card. Your payment is credited to your Credit Card account on \
            15/03/2026.\n\n.";
        let result = layer
            .extract(&pool, "IndusInd Bank", body)
            .await
            .expect("credit_card_bill_payment_thank_you pattern must match");
        assert_eq!(result.amount_minor, Some(13400));
        assert_eq!(
            result.merchant_raw,
            Some("IndusInd Bank Credit Card".to_string())
        );
        assert_eq!(result.direction, Some("credit".to_string()));
        assert!(
            result.masked_identifier.is_none(),
            "this narration genuinely has no card digits anywhere in the source"
        );
    }

    #[tokio::test]
    async fn test_hdfc_neft_transfer_to_payee() {
        let pool = dummy_pool();
        let layer = BankTemplateLayer;
        let body = "Thank you for banking with HDFC Bank.\n\nRs. 70000 has been deducted from \
            your HDFC Bank account ending in XX4691 for a transfer to payee Rina Rawal SBI \
            Account via NEFT using HDFC Bank Online Banking.";
        let result = layer
            .extract(&pool, "HDFC Bank", body)
            .await
            .expect("neft_transfer_to_payee pattern must match");
        assert_eq!(result.amount_minor, Some(7_000_000));
        assert_eq!(
            result.merchant_raw,
            Some("Rina Rawal SBI Account".to_string())
        );
        assert_eq!(result.direction, Some("debit".to_string()));

        let self_transfer_body = "Thank you for banking with HDFC Bank.\n\nRs. 82164 has been \
            deducted from your HDFC Bank account ending in XX4691 for a transfer to payee Self \
            Transfer via NEFT using HDFC Bank Online Banking.";
        let self_result = layer
            .extract(&pool, "HDFC Bank", self_transfer_body)
            .await
            .expect("neft_transfer_to_payee pattern must match the self-transfer wording too");
        assert_eq!(self_result.merchant_raw, Some("Self Transfer".to_string()));
    }

    #[tokio::test]
    async fn test_hdfc_neft_credit_cr_ifsc_name() {
        let pool = dummy_pool();
        let layer = BankTemplateLayer;
        let body = "Greetings from HDFC Bank!\n\nRs.INR 10,000.00 has been successfully added \
            to your account ending XX4691 from NEFT Cr-SBIN0010341-RINA RAWAL-Aditya \
            Rawal-SBIN426064133764 on 05-MAR-2026.";
        let result = layer
            .extract(&pool, "HDFC Bank", body)
            .await
            .expect("neft_credit_cr_ifsc_name pattern must match");
        assert_eq!(result.amount_minor, Some(1_000_000));
        assert_eq!(result.merchant_raw, Some("RINA RAWAL".to_string()));
        assert_eq!(result.direction, Some("credit".to_string()));
    }

    #[tokio::test]
    async fn test_hdfc_account_credit_ref_code_merchant() {
        let pool = dummy_pool();
        let layer = BankTemplateLayer;
        let body = "Greetings from HDFC Bank!\n\nRs.INR 1,12,866.00 has been successfully added \
            to your account ending XX4691 from A2AINT01-THEMATHCOMPANY PRIVATE \
            LIMITED-Salary-SalaryMar26 on 30-MAR-2026.";
        let result = layer
            .extract(&pool, "HDFC Bank", body)
            .await
            .expect("account_credit_ref_code_merchant pattern must match");
        assert_eq!(result.amount_minor, Some(11_286_600));
        assert_eq!(
            result.merchant_raw,
            Some("THEMATHCOMPANY PRIVATE LIMITED".to_string())
        );
        assert_eq!(result.direction, Some("credit".to_string()));
    }

    #[test]
    fn test_parse_amount_trailing_stop_and_implausible_values() {
        assert_eq!(
            parse_amount("706.00."),
            Some(70600),
            "bank prose ends the amount with a full stop; the stray dot must not \
             fail the parse and drop the amount"
        );
        assert_eq!(parse_amount("1,020.00,"), Some(102000));
        assert_eq!(
            parse_amount("99999999999999999999"),
            None,
            "a float-to-int cast saturates, so an out-of-range figure must be \
             rejected rather than booked as i64::MAX paise"
        );
        assert_eq!(
            parse_amount(".50"),
            Some(50),
            "a leading dot is the decimal point"
        );
        assert_eq!(parse_amount("Ref"), None);
        assert_eq!(
            parse_amount("Rs.2500.00"),
            Some(250000),
            "the dot in \"Rs.\" is punctuation, not a decimal point; kept, it \
             leaves \".2500.00\" and the whole figure is dropped"
        );
        assert_eq!(parse_amount("INR.1,020.00"), Some(102000));
    }

    #[tokio::test]
    async fn test_nlp_balance_survives_a_currency_prefix_ending_in_a_dot() {
        let pool = dummy_pool();
        let body = "Rs 500.00 debited from HDFC Bank A/c ending 1234 at Amazon on 25-May-23 \
                    Avl Bal Rs.2500.00";
        let result = NlpLayer.extract(&pool, "HDFC Bank", body).await.unwrap();
        assert_eq!(
            result.balance_after,
            Some(250000),
            "\"Rs.2500.00\" is one token, so the prefix's full stop reaches \
             parse_amount and silently dropped the balance"
        );
    }

    #[test]
    fn test_normalize_direction_abbreviations_are_not_prefixes() {
        for w in ["Crest Hotel", "Cristiano", "Dropbox", "Drone Services"] {
            assert_eq!(
                normalize_direction(w),
                None,
                "{w:?} merely starts with cr/dr; a fabricated direction is worse \
                 than none because the field then looks confidently populated"
            );
        }
        assert_eq!(normalize_direction("CR.").as_deref(), Some("credit"));
        assert_eq!(normalize_direction("dr.").as_deref(), Some("debit"));
    }

    #[test]
    fn test_template_hash_ignores_edge_whitespace() {
        assert_eq!(
            compute_template_hash("Rs 500 spent at SWIGGY"),
            compute_template_hash("\n  Rs 500 spent at SWIGGY  \n\n"),
            "MIME-to-text conversion varies the edge whitespace between two \
             renderings of one template; an unstable hash orphans the overrides \
             taught against it"
        );
    }

    #[tokio::test]
    async fn a_learned_last4_rule_capturing_no_digits_is_dropped() {
        let pool = dummy_migrated_pool().await;
        let body = "Rs 500.00 spent on your Card ending 1234 at Amazon on 01/07/26";
        {
            let conn = pool.get().await.unwrap();
            let b = body.to_string();
            conn.interact(move |c| {
                seed_rule(
                    c,
                    "HDFC Bank",
                    "last4",
                    &b,
                    serde_json::json!({"regex": r"at\s+([A-Za-z]+)", "capture_group": 1}),
                    "active",
                )
            })
            .await
            .unwrap();
        }

        let mut result = ExtractionResult::default();
        let fired = apply_learned_fields(&pool, "HDFC Bank", body, "email", &mut result).await;

        assert!(!fired);
        assert_eq!(
            result.masked_identifier, None,
            "a last4 with no digits and no VPA handle keys a phantom instrument, \
             and it would beat the correctly-read digits because \
             apply_instrument_signals only fills fields still empty"
        );
    }

    #[test]
    fn test_currency_amount_regex_ignores_rs_inside_a_word() {
        let (prefix_re, _) = generic_currency_amount_regexes();
        let caps = prefix_re
            .captures("Your Rewards 500 points. Rs 250.00 debited at Zomato.")
            .expect("the real amount must still match");
        assert_eq!(
            parse_amount(caps.get(2).unwrap().as_str()),
            Some(25000),
            "unanchored, `rs` matches inside \"Rewards\" and a loyalty balance \
             becomes the transaction amount"
        );
        assert!(
            prefix_re
                .captures("Cards 1234 and 5678 are active.")
                .is_none(),
            "a card number is not an amount"
        );
    }

    #[test]
    fn test_debit_card_not_misread_as_credit_card_via_account() {
        let signals = extract_instrument_signals(
            "HDFC Bank",
            "Rs 500.00 spent on your Debit Card ending 1234 from your account on 25-May-23.",
        );
        assert_eq!(
            signals.instrument_type,
            Some("debit_card".to_string()),
            "\"cc\" as a substring hits the \"account\" in every debit-card alert"
        );
    }

    #[test]
    fn test_detect_channel_does_not_invent_channels_from_ordinary_words() {
        let obs = ExtractionResult::default();
        assert_eq!(
            detect_channel(&obs, "Ecstatic news! Rs 500 credited by Nachiket."),
            None,
            "\"ecs\" and \"nach\" as substrings invent a mandate channel out of prose"
        );
    }

    #[tokio::test]
    async fn a_learned_direction_rule_is_normalized_to_the_two_ledger_values() {
        let pool = dummy_migrated_pool().await;
        let body = "Rs 500.00 was credited to your account on 01/07/26";
        {
            let conn = pool.get().await.unwrap();
            let b = body.to_string();
            conn.interact(move |c| {
                seed_rule(
                    c,
                    "HDFC Bank",
                    "direction",
                    &b,
                    serde_json::json!({"regex": "(credited)", "capture_group": 1}),
                    "active",
                );
                seed_rule(
                    c,
                    "HDFC Bank",
                    "currency",
                    &b,
                    serde_json::json!({"regex": r"(Rs)\s", "capture_group": 1}),
                    "active",
                );
            })
            .await
            .unwrap();
        }

        let mut result = ExtractionResult::default();
        apply_learned_fields(&pool, "HDFC Bank", body, "email", &mut result).await;

        assert_eq!(
            result.direction.as_deref(),
            Some("credit"),
            "every consumer compares direction against exactly \"debit\"/\"credit\", \
             so the raw capture \"credited\" matches nothing downstream"
        );
        assert_eq!(
            result.currency.as_deref(),
            Some("INR"),
            "\"Rs\" upper-cased is \"RS\", which is not an ISO code"
        );
    }

    #[tokio::test]
    async fn a_learned_direction_rule_with_unrecognised_wording_is_dropped() {
        let pool = dummy_migrated_pool().await;
        let body = "Rs 500.00 transacted on your account on 01/07/26";
        {
            let conn = pool.get().await.unwrap();
            let b = body.to_string();
            conn.interact(move |c| {
                seed_rule(
                    c,
                    "HDFC Bank",
                    "direction",
                    &b,
                    serde_json::json!({"regex": "(transacted)", "capture_group": 1}),
                    "active",
                )
            })
            .await
            .unwrap();
        }

        let mut result = ExtractionResult::default();
        let fired = apply_learned_fields(&pool, "HDFC Bank", body, "email", &mut result).await;

        assert!(!fired);
        assert_eq!(
            result.direction, None,
            "an unrecognised capture must leave the field untouched rather than \
             writing a value nothing downstream matches"
        );
    }

    #[test]
    fn test_normalize_direction_wordings() {
        for w in ["credited", "CREDIT", "Cr", "credit", "received"] {
            assert_eq!(normalize_direction(w).as_deref(), Some("credit"), "{w}");
        }
        for w in ["debited", "DEBIT", "Dr", "spent", "paid", "withdrawn"] {
            assert_eq!(normalize_direction(w).as_deref(), Some("debit"), "{w}");
        }
        assert_eq!(normalize_direction("transacted"), None);
        assert_eq!(normalize_direction(""), None);
    }

    #[tokio::test]
    async fn test_nlp_first_balance_wins_over_a_later_reward_balance() {
        let pool = dummy_pool();
        let body = "Rs 500.00 debited from HDFC Bank A/c ending 1234 at Amazon on 25-May-23. \
                    Bal: 1000.00. Reward Bal: 0.00";
        let result = NlpLayer.extract(&pool, "HDFC Bank", body).await.unwrap();
        assert_eq!(
            result.balance_after,
            Some(100000),
            "a trailing rewards balance must not overwrite the account balance \
             the message already stated"
        );
    }

    #[tokio::test]
    async fn test_nlp_balance_reads_past_a_run_of_filler_tokens() {
        let pool = dummy_pool();
        let body =
            "Rs 500.00 debited from HDFC Bank A/c ending 1234 at Amazon on 25-May-23 Avl Bal is Rs 2500.00";
        let result = NlpLayer.extract(&pool, "HDFC Bank", body).await.unwrap();
        assert_eq!(
            result.balance_after,
            Some(250000),
            "skipping only one filler token leaves the parse pointed at \"Rs\""
        );
    }

    #[test]
    fn test_card_mask_does_not_bridge_a_sentence_boundary_into_a_date() {
        let signals = extract_instrument_signals(
            "HDFC Bank",
            "Thank you for using your HDFC Bank Credit Card. Transaction on 25-May-23 at Amazon.",
        );
        assert_eq!(
            signals.masked_identifier, None,
            "a sentence-ending full stop is not a mask; \"25\" here would key a \
             phantom instrument"
        );

        // The ellipsis mask the gap exists for must still work.
        assert_eq!(
            extract_instrument_signals("HDFC Bank", "card ending ...1234").masked_identifier,
            Some("1234".to_string())
        );
    }

    #[test]
    fn test_parse_learned_event_time_bounds_and_scales() {
        assert_eq!(
            parse_learned_event_time("123"),
            None,
            "a short numeric capture -- an auth code, an installment count -- \
             must be rejected, not booked as a 1970 event time"
        );
        assert_eq!(parse_learned_event_time("0"), None);

        assert_eq!(
            parse_learned_event_time("1700000000"),
            Some((1_700_000_000, false))
        );
        assert_eq!(
            parse_learned_event_time("1700000000000"),
            Some((1_700_000_000, false)),
            "a millisecond epoch must be rescaled, not read as the year 55000"
        );
        assert_eq!(
            parse_learned_event_time("533264925852"),
            None,
            "a UPI reference number is not a timestamp"
        );
        assert_eq!(
            parse_learned_event_time("2026-03-30 00:00:00"),
            Some((ymd_ts(2026, 3, 30), false)),
            "this is the shape the learning path writes back as a corrected date"
        );
    }

    #[test]
    fn test_iso_dates_parse_without_breaking_day_first_dates() {
        assert_eq!(
            parse_date_generic("2026-03-30").map(|p| p.timestamp),
            Some(ymd_ts(2026, 3, 30))
        );
        assert_eq!(
            parse_date_generic("23-12-25").map(|p| p.timestamp),
            Some(ymd_ts(2025, 12, 23)),
            "ISO parsing must stay last, or this reads as year 23"
        );
    }

    #[tokio::test]
    async fn test_nlp_footer_does_not_flip_direction_or_date() {
        let pool = dummy_pool();
        let body = "Rs 500.00 debited from HDFC Bank A/c ending 1234 at Amazon on 25-May-23 \
                    Bal Rs 1000.00. If the amount is not credited back, report on 01-Jan-24.";
        let result = NlpLayer.extract(&pool, "HDFC Bank", body).await.unwrap();
        assert_eq!(
            result.direction,
            Some("debit".to_string()),
            "a closing disclaimer must not flip the direction the message stated"
        );
        assert_eq!(result.event_time, Some(ymd_ts(2023, 5, 25)));
    }

    #[test]
    fn test_fx_transaction_is_not_flagged_as_amount_disagreement() {
        let body = "Acct XX1234 debited USD 50.00 (INR 4150.50) on 25-May-23 at Netflix.";
        let mut obs = ExtractionResult {
            amount_minor: Some(415050),
            original_amount_minor: Some(5000),
            confidence_score: Some(LAYER12_CONFIDENCE),
            ..Default::default()
        };
        apply_amount_cross_check(&mut obs, body);
        assert_eq!(
            obs.confidence_score,
            Some(LAYER12_CONFIDENCE),
            "the first amount in an FX body is the foreign one; agreeing with it \
             is agreement, not a disagreement to downgrade for"
        );
    }

    #[tokio::test]
    async fn test_layer5_result_carries_a_confidence_score() {
        let anchor = chrono::NaiveDate::from_ymd_opt(2023, 5, 25).unwrap();
        let entry_date = chrono::NaiveDate::from_ymd_opt(2023, 5, 24).unwrap();
        let pool = setup_crossref_db(vec![crossref_entry("se_1", entry_date, 150000, None)]).await;

        let obs = Layer5CrossrefLayer
            .extract(
                &pool,
                "HDFC Bank",
                "Rs 1500.00 spent on your HDFC Bank credit card ending 1234.",
                Some(anchor),
            )
            .await
            .expect("the unique statement entry must complete the extraction");

        assert_eq!(
            obs.confidence_score,
            Some(LAYER5_CONFIDENCE),
            "an unset confidence reads downstream as not confident at all, so a \
             statement-backed result would never auto-resolve"
        );
    }

    #[tokio::test]
    async fn the_rule_authored_for_this_template_wins_over_another_live_variant() {
        let pool = dummy_migrated_pool().await;
        let this_shape = "Rs 500 spent at ALPHA STORE on 01/07/26";
        let other_shape = "Rs 500 spent at BETA STORE on 01/07/26 -- thank you for banking.";

        let conn = pool.get().await.unwrap();
        let (a, b) = (this_shape.to_string(), other_shape.to_string());
        conn.interact(move |c| {
            seed_rule(
                c,
                "HDFC Bank",
                "merchant",
                &a,
                serde_json::json!({"regex": r"at\s+(.{1,80}?)\s+on", "capture_group": 1}),
                "active",
            );
            seed_rule(
                c,
                "HDFC Bank",
                "merchant",
                &b,
                serde_json::json!({"regex": r"spent\s+at\s+(\S+)", "capture_group": 1}),
                "active",
            );
        })
        .await
        .unwrap();
        drop(conn);

        let mut result = ExtractionResult::default();
        apply_learned_fields(&pool, "HDFC Bank", this_shape, "email", &mut result).await;

        assert_eq!(
            result.merchant_raw.as_deref(),
            Some("ALPHA STORE"),
            "both variants match this body, so without a deterministic ranking the \
             winner is whatever order SQLite happened to return"
        );
    }

    #[tokio::test]
    async fn test_hdfc_credit_card_debit_to_upi_handle() {
        let pool = dummy_pool();
        let layer = BankTemplateLayer;
        let body = "Rs.400.00 has been debited from your HDFC Bank RuPay Credit Card XX8256 to \
            paytm-81642725@ptys SUVIM CARE on 22-03-26. Your UPI transaction reference number \
            is 644708657028.";
        let result = layer
            .extract(&pool, "HDFC Bank", body)
            .await
            .expect("credit_card_debit_to_upi_handle pattern must match");
        assert_eq!(result.amount_minor, Some(40000));
        assert_eq!(result.merchant_raw, Some("SUVIM CARE".to_string()));
        assert_eq!(result.direction, Some("debit".to_string()));
        assert_eq!(result.reference_id, Some("644708657028".to_string()));
    }
}
