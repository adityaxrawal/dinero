//! Vocabulary helpers shared by the extraction layers.
//!
//! Label matching locates a field's caption within message text, and the
//! stopword check rejects merchant names that are only filler words -- a
//! "merchant" extracted as "payment to" is a failed extraction, not a merchant,
//! and catching that here prevents it reaching the ledger.
pub const CREDIT_VERBS: &[&str] = &[
    "credited",
    "received",
    "refund",
    "refunded",
    "deposited",
    "reversal",
    "reversed",
    "added",
    "returned",
    "cashback",
];

pub const DEBIT_VERBS: &[&str] = &[
    "debited",
    "spent",
    "paid",
    "withdrawn",
    "payment",
    "sent",
    "deducted",
    "purchase",
    "purchased",
    "charged",
    "billed",
];

pub const CREDIT_PHRASES: &[&str] = &["transfer from", "money received", "credited with"];
pub const DEBIT_PHRASES: &[&str] = &["transfer to", "debited with", "will be debited"];

pub const MERCHANT_STOPWORDS: &[&str] = &[
    "your",
    "you",
    "us",
    "the",
    "this",
    "that",
    "please",
    "immediately",
    "block",
    "call",
    "calling",
    "contact",
    "sms",
    "click",
    "here",
    "report",
    "dispute",
    "care",
    "customer",
    "helpdesk",
    "not",
    "and",
    "or",
    "for",
    "with",
    "by",
    "to",
    "from",
    "at",
    "on",
    "was",
    "is",
    "card",
    "account",
    "transaction",
    "number",
    "if",
    "do",
    "did",
    "have",
    "has",
    "been",
    "a",
    "an",
    "of",
    "in",
    "immediate",
    "assistance",
    "help",
    "service",
    "team",
    "app",
    "visit",
    "visiting",
    "toll",
    "free",
    "hotline",
    "no",
    "our",
    "we",
    "kindly",
    "reply",
    "inform",
    "informing",
    "advise",
    "advised",
    "wish",
    "notice",
    "note",
    "would",
    "like",
    "hereby",
    "bring",
    "bank",
    "banking",
    "card",
    "cards",
    "credit",
    "debit",
    "rupay",
    "visa",
    "mastercard",
    "amex",
    "upi",
    "vpa",
    "neft",
    "imps",
    "rtgs",
    "ach",
    "nach",
    "emi",
    "atm",
    "pos",
    "transaction",
    "transactions",
    "txn",
    "trxn",
    "amount",
    "inr",
    "rs",
    "usd",
    "payment",
    "paid",
    "pay",
    "spent",
    "debited",
    "credited",
    "purchase",
    "transfer",
    "balance",
    "available",
    "avl",
    "limit",
    "ref",
    "reference",
    "biller",
    "name",
    "pot",
    "superpot",
    "using",
    "ending",
    "ends",
    "xx",
    "xxxx",
    "processing",
    "fee",
    "charges",
    "towards",
    "successful",
    "successfully",
    "update",
    "alert",
    "details",
    "detail",
    "info",
    "using",
];

pub const MIN_MERCHANT_NAME_LEN: usize = 3;

/// Whether a merchant candidate is nothing but filler words.
///
/// A "merchant" extracted as "payment to" is a failed extraction, not a merchant.
/// Catching that here keeps meaningless names out of the ledger, where they would
/// otherwise aggregate into a nonsense spending category.
pub fn is_stopword_only_merchant(candidate: &str) -> bool {
    let mut saw_token = false;
    for tok in candidate.split_whitespace() {
        saw_token = true;
        let cleaned = tok.trim_matches(|c: char| !c.is_alphanumeric());
        if cleaned.is_empty() {
            continue;
        }
        if !MERCHANT_STOPWORDS.contains(&cleaned.to_lowercase().as_str()) {
            return false;
        }
    }
    saw_token
}

pub const MERCHANT_LABEL_STRICT: &[&str] = &[
    "towards",
    "paid to",
    "purchased at",
    "txn at",
    "info",
    "beneficiary",
    "in favor of",
    "merchant name",
    "merchant",
];

pub const MERCHANT_LABEL_AMBIGUOUS: &[&str] = &["at", "to", "from", "for", "by"];

/// Matches a multi-word field label at a token position.
///
/// Returns the label's length so the caller can resume scanning after it. Works
/// over tokens rather than raw text because bank emails vary their whitespace,
/// and a substring match would also fire inside longer unrelated words.
pub fn match_label_at(lower_tokens: &[String], i: usize, keywords: &[&str]) -> Option<usize> {
    'keyword: for kw in keywords {
        let words: Vec<&str> = kw.split(' ').collect();
        if i + words.len() > lower_tokens.len() {
            continue;
        }
        for (offset, word) in words.iter().enumerate() {
            let candidate = lower_tokens[i + offset].trim_end_matches(':');
            if candidate != *word {
                continue 'keyword;
            }
        }
        return Some(words.len());
    }
    None
}
