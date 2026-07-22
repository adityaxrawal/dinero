//! Shared direction/merchant-label keyword lexicon for Layer 3
//! (`GenericRegexLayer`, regex-based) and Layer 4 (`NlpLayer`,
//! token-based).
//!
//! Before this module, both layers independently reimplemented near-
//! identical keyword lists. They drifted: the `ladder.rs` "Cluster D/E/F/G"
//! comment trail documents real-world bank emails that broke Layer 3's
//! merchant-label matching, each fixed by adding a keyword to Layer 3's
//! list alone -- Layer 4 (which only runs after Layer 3 has already
//! failed) never received any of those fixes, so it silently carried the
//! same bugs Layer 3 had already solved. One shared source of truth here
//! means a future fix lands in both layers automatically.

/// Single-word credit-direction verbs, shared verbatim by both layers.
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

/// Single-word debit-direction verbs, shared verbatim by both layers.
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

/// Multi-word direction phrases. Regex-only (Layer 3): Layer 4's
/// token-by-token walk has no multi-word match at all, so these can't be
/// ported there -- a structural difference between the two layers, not a
/// drift risk the way the single-word lists were.
pub const CREDIT_PHRASES: &[&str] = &["transfer from", "money received", "credited with"];
pub const DEBIT_PHRASES: &[&str] = &["transfer to", "debited with", "will be debited"];

/// Boilerplate/instruction words that show up in bank-alert disclaimer
/// footers ("please **block your** card immediately **by** calling...",
/// "write **to us** at...", "**contact** customer **care**") -- every one of
/// these also happens to be, or sit immediately next to, an ambiguous
/// merchant-label keyword (`MERCHANT_LABEL_AMBIGUOUS`), so a footer sentence
/// can satisfy the same "at/to/from/for/by + 2-40 chars" shape a real
/// merchant label does. A genuine merchant name is never composed *entirely*
/// of these filler words, so `is_invalid_merchant` rejects a candidate only
/// when every one of its tokens is on this list, rather than blocklisting
/// specific phrases (which breaks the moment a bank rewords its footer).
pub const MERCHANT_STOPWORDS: &[&str] = &[
    "your", "you", "us", "the", "this", "that", "please", "immediately", "block", "call",
    "calling", "contact", "sms", "click", "here", "report", "dispute", "care", "customer",
    "helpdesk", "not", "and", "or", "for", "with", "by", "to", "from", "at", "on", "was", "is",
    "card", "account", "transaction", "number", "if", "do", "did", "have", "has", "been", "a",
    "an", "of", "in", "immediate", "assistance", "help", "service", "team", "app", "visit",
    "visiting", "toll", "free", "hotline", "no", "our", "we", "kindly", "reply",
];

/// `true` when `candidate` is nothing but [`MERCHANT_STOPWORDS`] -- e.g. "to
/// **block your** card" matching the ambiguous "to" label and capturing
/// "block your" before the `card` terminator. Real merchant names always
/// carry at least one token that isn't generic instruction filler.
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

/// Unambiguous merchant-label keywords/phrases, tried before
/// [`MERCHANT_LABEL_AMBIGUOUS`] in both layers (Doc 30 TASK-TXN-004 /
/// Cluster E/G: "at|to|from|for|by" can also label the *source* instrument
/// -- "debited from your HDFC Bank Credit Card" -- not the counterparty, so
/// the unambiguous set must win when both are present in the same body).
/// Deliberately plain words with no baked-in punctuation ("info", not
/// "info:") -- both consumers handle a trailing colon on the last matched
/// word themselves (Layer 3 via `MERCHANT_TERMINATOR`'s leading `:?`, Layer
/// 4 via [`match_label_at`]'s per-token trim), so a keyword needs exactly
/// one plain-text form here.
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

/// Ambiguous merchant-label keywords -- only reached once
/// [`MERCHANT_LABEL_STRICT`] has been tried and failed.
pub const MERCHANT_LABEL_AMBIGUOUS: &[&str] = &["at", "to", "from", "for", "by"];

/// Checks whether one of `keywords` (each a lowercase, space-separated
/// phrase -- e.g. `"paid to"` or `"merchant"`) matches starting at
/// `lower_tokens[i]`. A trailing `:` on each compared token is stripped
/// before comparing (Cluster G: "Payment from:   NAME", "Merchant Name:
/// RAZ*SWIGGY" -- the colon can land on either the single-word keyword
/// itself or the last word of a multi-word one). Returns the number of
/// tokens the match consumed so the caller can advance past the label,
/// or `None` if no keyword matches at this position.
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
