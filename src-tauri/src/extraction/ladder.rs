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

pub fn evaluate_gate3(
    has_amount: bool,
    has_entity: bool,
    has_balance: bool,
    has_instrument: bool,
) -> bool {
    (has_amount && has_entity && has_instrument) || has_balance
}

impl ExtractionResult {
    /// Gate 3: whether extraction recovered enough to record a transaction.
    ///
    /// Two ways to pass. Either the full trio of amount, counterparty and instrument
    /// is present, or the message carries a balance -- a balance-only alert is
    /// legitimate data even though it describes no transaction.
    ///
    /// Failing this gate is what routes an observation to the unassigned queue rather
    /// than into the ledger.
    pub fn passes_gate3(&self) -> bool {
        let has_instrument = self.instrument_type.is_some()
            && self.issuer_name.is_some()
            && self.masked_identifier.is_some();
            
        evaluate_gate3(
            self.amount_minor.is_some(),
            self.merchant_raw.is_some(),
            self.balance_after.is_some(),
            has_instrument,
        )
    }

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

    if candidate_lower == "bank" || candidate_lower == "alerts" {
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
    // Fill-not-overwrite for all signal fields: a field already extracted by an
    // earlier layer must not be erased when the signal detector returns None.
    // issuer_name previously used unconditional assignment, breaking this
    // invariant and silently voiding the instrument triple → Gate 3 failure.
    obs.issuer_name = obs.issuer_name.take().or(signals.issuer_name);
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
pub(crate) struct DateParseResult {
    pub(crate) timestamp: i64,
    pub(crate) ambiguous: bool,
}

const NUMERIC_AMBIGUOUS_FORMATS: &[&str] =
    &["%d/%m/%Y", "%d-%m-%Y", "%m-%d-%Y", "%d/%m/%y", "%d-%m-%y"];

/// Parses a date in whichever format the bank used.
///
/// Reports how the date was interpreted alongside the value, because
/// day-first and month-first formats are genuinely ambiguous below the
/// thirteenth -- the caller needs that uncertainty rather than a confident guess.
pub(crate) fn parse_date_generic(s: &str) -> Option<DateParseResult> {
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
    pub pipeline: Option<crate::llm_pipeline::LlmPipeline>,
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

        let engine = crate::extraction::llm::LlmEngine::new(app_dir, &model_id, self.pipeline.clone());
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
                Layer6Outcome::TimedOut | Layer6Outcome::Failed | Layer6Outcome::Rejected | Layer6Outcome::NotATransaction => None,
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
        obs.emi_installment_number = obs.emi_installment_number.take().or(Some(number));
        obs.emi_total_installments = obs.emi_total_installments.take().or(Some(total));
        obs.emi_original_amount_minor = obs.emi_original_amount_minor.take().or_else(|| {
            crate::extraction::emi_detector::detect_emi_original_amount_minor(body)
        });
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
    obs.channel = obs.channel.take().or_else(|| detect_channel(obs, body));
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
    trace: &mut crate::logging::EmailTrace,
) -> Result<Option<ExtractionResult>> {
    let layers: [&dyn ExtractionLayer; 4] = [
        &LearnedFieldLayer,
        &BankTemplateLayer,
        &GenericRegexLayer,
        &NlpLayer,
    ];

    for layer in layers {
        let layer_name = layer.layer_name();
        trace.info(format!("    ↳ 🪜 Executing Extraction Layer: [{}]", layer_name));
        if let Some(mut obs) = layer.extract(pool, bank_name, body).await {
            if obs.is_valid() {
                finalize_result(pool, bank_name, body, internal_date, &mut obs).await;
                trace.info(format!(
                    "    ↳ ✅ Extraction Succeeded at Layer: [{}]. \n       Extracted: Amount: {:?}, Merchant: {:?}, Instrument: {:?}",
                    layer_name,
                    obs.amount_minor,
                    obs.merchant_raw,
                    obs.instrument_type
                ));
                return Ok(Some(obs));
            } else {
                trace.info(format!(
                    "    ↳ ⚠️ Layer [{}] produced incomplete results (missing mandatory fields). Trying next...",
                    layer_name
                ));
            }
        } else {
            if layer_name == "learned_fields" {
                trace.info(format!("    ↳ ⏭️ Layer [{}] skipped (no learned rules).", layer_name));
            } else if layer_name == "bank_templates" && !bank_has_template(bank_name) {
                trace.info(format!("    ↳ ⏭️ Layer [{}] skipped (no template for {}).", layer_name, bank_name));
            } else {
                trace.info(format!("    ↳ ❌ Layer [{}] failed to match or extract anything.", layer_name));
            }
        }
    }

    let anchor_date = internal_date
        .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0).map(|dt| dt.naive_utc().date()));
    trace.info("    ↳ 🪜 Executing Extraction Layer: [layer5_statement_crossref]");
    if let Some(mut crossref_result) = Layer5CrossrefLayer
        .extract(pool, bank_name, body, anchor_date)
        .await
    {
        if crossref_result.is_valid() {
            finalize_result(pool, bank_name, body, internal_date, &mut crossref_result).await;
            trace.info(format!(
                "    ↳ ✅ Extraction Succeeded at Layer: [layer5_statement_crossref]. \n       Extracted: Amount: {:?}, Merchant: {:?}, Instrument: {:?}",
                crossref_result.amount_minor,
                crossref_result.merchant_raw,
                crossref_result.instrument_type
            ));
            return Ok(Some(crossref_result));
        }
    }
    trace.info("    ↳ ❌ Layer [layer5_statement_crossref] failed to extract.");

    if !llm_eligible {
        trace.info("    ↳ ⏭️ Layer 6 (LLM) skipped — Device not eligible for local inference.");
        return Ok(None);
    }

    let layer6 = Layer6LlmLayer {
        app_dir: app_dir.clone(),
        fallback_event_time: internal_date,
        pipeline: None, // Used in historical scan where pipeline may not be initialized
    };
    let layer6_outcome = layer6.run(pool, bank_name, body).await;
    match layer6_outcome {
        Layer6Outcome::Extracted(boxed_llm_result) => {
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
            Ok(None)
        }
        Layer6Outcome::TimedOut => {
            *layer6_timed_out = true;
            Err(anyhow::anyhow!("Layer 6 LLM extraction timed out"))
        }
        Layer6Outcome::Failed => Err(anyhow::anyhow!("Layer 6 LLM extraction failed due to infrastructure error")),
        Layer6Outcome::Rejected => Ok(None),
        Layer6Outcome::NotATransaction => {
            trace.info("    ↳ ⏭️ Layer 6 (LLM) explicitly classified this as a non-transaction email.");
            Ok(None)
        }
    }
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
#[path = "tests/ladder_tests.rs"]
mod tests;
