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
    "deposited",
    "reversal",
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
];

/// Multi-word direction phrases. Regex-only (Layer 3): Layer 4's
/// token-by-token walk has no multi-word match at all, so these can't be
/// ported there -- a structural difference between the two layers, not a
/// drift risk the way the single-word lists were.
pub const CREDIT_PHRASES: &[&str] = &["transfer from"];
pub const DEBIT_PHRASES: &[&str] = &["transfer to"];

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
