//! Classifies message content as transactional, mandate-related, or neither.
//!
//! Runs after sender verification. A verified bank sends statements, alerts,
//! marketing and service notices alike, so establishing the sender is genuine is
//! not the same as establishing the message contains a transaction.
use regex::Regex;
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentClass {
    TransactionAlert,
    BalanceUpdate,
    StatementEmail,
    MandateRegistration,
    MandateCancellation,
    Noise,
    Otp,
    Kyc,
    Marketing,
    Reminder,
    Unknown,
}

pub struct ContentClassifier;

/// Currency-amount pattern, compiled once.
///
/// Three shapes, because Indian bank templates use all of them: symbol-first
/// (`₹300.00`), code-first with an optional period (`Rs. 500`, `INR. 0.00`), and
/// amount-first (`0.00 INR`). The code-first arm anchors on a word boundary so
/// the "rs" inside "hours 24" cannot pose as an amount.
fn amount_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)(?:₹\s*[\d,]+(?:\.\d{1,2})?|\b(?:rs|inr)\.?\s*[\d,]+(?:\.\d{1,2})?|[\d,]+(?:\.\d{1,2})?\s*(?:₹|\b(?:rs|inr)\b))",
        )
        .unwrap()
    })
}

/// Whether the content contains something shaped like a monetary amount.
fn has_amount_pattern(content: &str) -> bool {
    amount_regex().is_match(content)
}

/// Subject tokens for the Gmail rescue pass -- retrieval only, not classification.
///
/// Gmail reads `subject:(a b)` as "a AND b", so a phrase is never broader than
/// its rarest word: `subject:(credited)` already returns every "money credited"
/// subject, for a fraction of the query budget this list shares with the
/// sender chunks. Single words where they are distinctive, phrases where each
/// word alone would drag in the whole mailbox.
///
/// Precision is not this list's job -- the gates downstream do that. A phrase
/// the classifier accepts but no token here can reach is a transaction that is
/// never fetched at all, which `subject_terms_cover_classifier_phrases` guards.
pub const RESCUE_SUBJECT_TERMS: &[&str] = &[
    "spent",
    "debited",
    "debit of",
    "credited",
    "credit of",
    "withdrawn",
    "withdrawal",
    "deducted",
    "paid",
    "payment",
    "purchase",
    "purchased",
    "transaction",
    "txn",
    "transferred",
    "transfer of",
    "charged",
    "swiped",
    "card used",
    "card was used",
    "recharge successful",
    "received",
    "refund",
    "refunded",
    "reversal",
    "reversed",
    "deposited",
    "money added",
    "available balance",
    "available bal",
    "avl balance",
    "avl bal",
    "account balance",
    "a/c balance",
    "closing balance",
    "balance update",
    "updated balance",
    "balance in your account",
    "account update",
    "upi payment",
];

/// Language for a transaction that has already settled.
///
/// Required alongside an amount, because an amount alone appears in marketing and
/// statements too. Both signals together are what distinguish a transaction.
///
/// Every entry is anchored to a neighbouring word on purpose. Bare "paid",
/// "payment" or "transaction" would swallow "not paid yet", "payment due" and
/// "report an unauthorised transaction", which is how a reminder ends up in the
/// ledger as a charge.
const TRANSACTION_VERBS: &[&str] = &[
    // Debits.
    "spent",
    "debited",
    "debit of",
    "withdrawn",
    "withdrawal of",
    "deducted",
    "you paid",
    "you have paid",
    "you've paid",
    "paid to",
    "bill paid",
    "payment of",
    "payment made",
    "payment done",
    "payment successful",
    "payment was successful",
    "purchase of",
    "purchase at",
    "purchased at",
    "transaction of",
    "transaction alert",
    "transaction successful",
    "transaction was successful",
    "txn of",
    "txn at",
    "transferred to",
    "money transferred",
    "transfer of",
    "charged to",
    "has been charged",
    "swiped at",
    "card used at",
    "card was used",
    "recharge successful",
    // Credits.
    "credited",
    "credit of",
    "payment received",
    "money received",
    "amount received",
    "funds received",
    "you received",
    "you have received",
    "refund of",
    "refunded",
    "refund processed",
    "reversal of",
    "reversed",
    "deposited",
    "money added",
];

/// Language for an alert that reports a balance without a transaction.
///
/// Checked only after the transaction verbs, because `BalanceUpdate` overwrites
/// the amount with zero downstream -- a real debit that also quotes the closing
/// balance must never land here.
const BALANCE_TERMS: &[&str] = &[
    "available balance",
    "available bal",
    "avl balance",
    "avl bal",
    "account balance",
    "a/c balance",
    "closing balance",
    "balance update",
    "updated balance",
    "balance in your account",
    "account update",
    "upi payment",
];

/// Phrases describing money that has not moved -- future tense or negated.
///
/// Removed from the text before any verb match rather than short-circuiting the
/// whole message, so a mandate pre-debit notice ("Rs 199 will be debited on the
/// 5th") stops reading as a settled debit while an alert that carries both a
/// warning and a real charge keeps the real one.
const UNSETTLED_PHRASES: &[&str] = &[
    "be debited",
    "be credited",
    "be deducted",
    "be charged",
    "be withdrawn",
    "be paid",
    "be refunded",
    "be reversed",
    "be transferred",
    "not debited",
    "not been debited",
    "not credited",
    "not been credited",
    "not received",
    "not paid",
    "not been paid",
];

/// Markers that a transaction was attempted and did not go through.
///
/// A declined charge quotes an amount and a debit verb exactly like a real one,
/// so nothing else in this file can tell them apart.
///
/// ponytail: whole-message check, so bank boilerplate that merely explains what
/// to do "if your payment failed" would also land here. Scope it to the
/// sentence around the amount if that template turns up.
const FAILURE_PHRASES: &[&str] = &[
    "transaction failed",
    "transaction has failed",
    "transaction was unsuccessful",
    "transaction declined",
    "transaction was declined",
    "payment failed",
    "payment has failed",
    "payment was unsuccessful",
    "payment declined",
    "debit failed",
    "autopay failed",
    "could not be processed",
    "unable to process your payment",
    "insufficient balance",
    "insufficient funds",
];

/// Whether the message announces a cancelled standing instruction.
fn is_mandate_cancellation(content: &str) -> bool {
    contains_any(
        content,
        &[
            "mandate cancelled",
            "mandate canceled",
            "mandate cancellation",
            "mandate stands cancelled",
            "mandate has been cancelled",
            "mandate revoked",
            "mandate withdrawn",
            "mandate deleted",
            "mandate suspended",
            "mandate terminated",
            "autopay deactivated",
            "autopay cancelled",
            "autopay canceled",
            "autopay stopped",
            "autopay disabled",
            "standing instruction cancelled",
            "standing instruction has been cancelled",
            "standing instruction revoked",
            "subscription cancelled",
        ],
    )
}

/// Whether the message announces a newly registered mandate.
///
/// Distinguished from a cancellation because they move a subscription in opposite
/// directions, and confusing them would leave a cancelled charge still predicted.
fn is_mandate_registration(content: &str) -> bool {
    contains_any(
        content,
        &[
            "mandate registered",
            "mandate registration",
            "mandate set at merchant",
            "mandate created",
            "mandate has been created",
            "mandate approved",
            "mandate authenticated",
            "mandate activated",
            "mandate is now active",
            "registration success",
            "autopay activated",
            "autopay enabled",
            "autopay set up",
            "autopay setup",
            "successful autopay transaction",
            "standing instruction registered",
            "standing instruction set",
        ],
    )
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| haystack.contains(n))
}

/// Strips future-tense and negated forms so only settled language remains.
fn settled_text(content: &str) -> String {
    let mut out = content.to_string();
    for phrase in UNSETTLED_PHRASES {
        if out.contains(phrase) {
            out = out.replace(phrase, " ");
        }
    }
    out
}

impl ContentClassifier {
    /// Classifies a message as transactional, mandate-related, or neither.
    ///
    /// Mandate cases are tested before the transactional ones, because a mandate
    /// notification also mentions an amount and would otherwise be misread as a
    /// charge that has already happened -- inventing spending that never occurred.
    pub fn classify(subject: &str, body: &str) -> ContentClass {
        let subject_lower = subject.to_lowercase();
        let body_lower = body.to_lowercase();

        let content = format!("{} {}", subject_lower, body_lower);

        if subject_lower.contains("otp")
            || subject_lower.contains("one time password")
            || subject_lower.contains("verification code")
        {
            return ContentClass::Otp;
        }

        if subject_lower.contains("kyc")
            || subject_lower.contains("know your customer")
            || subject_lower.contains("pan update")
            || subject_lower.contains("aadhaar")
        {
            return ContentClass::Kyc;
        }

        if subject_lower.contains("statement") || subject_lower.contains("e-statement") {
            return ContentClass::StatementEmail;
        }

        if is_mandate_cancellation(&content) {
            return ContentClass::MandateCancellation;
        }
        if is_mandate_registration(&content) {
            return ContentClass::MandateRegistration;
        }

        if subject_lower.contains("personal loan") || subject_lower.contains("loan offer") {
            return ContentClass::Marketing;
        }

        let settled = settled_text(&content);

        if contains_any(&settled, FAILURE_PHRASES) {
            return ContentClass::Noise;
        }

        let has_verb = contains_any(&settled, TRANSACTION_VERBS);
        let settled_transaction = has_amount_pattern(&content) && has_verb;

        if !settled_transaction
            && (subject_lower.contains("offer")
                || subject_lower.contains("exclusive")
                || subject_lower.contains("apply now")
                || subject_lower.contains("pre-approved")
                || subject_lower.contains("cashback"))
        {
            return ContentClass::Marketing;
        }

        if !has_verb
            && (subject_lower.contains("payment due")
                || subject_lower.contains("due date")
                || subject_lower.contains("reminder")
                || subject_lower.contains("overdue"))
        {
            return ContentClass::Reminder;
        }

        // Before the balance branch, and over the whole message rather than the
        // subject alone: `BalanceUpdate` zeroes the amount downstream, so a debit
        // whose subject only mentions the balance must still be caught here.
        if has_verb {
            return ContentClass::TransactionAlert;
        }

        if contains_any(&settled, BALANCE_TERMS) {
            return ContentClass::BalanceUpdate;
        }

        if subject_lower.contains("terms")
            || subject_lower.contains("conditions")
            || subject_lower.contains("important notice")
        {
            return ContentClass::Noise;
        }

        ContentClass::Unknown
    }

    /// Whether every phrase `classify` accepts can be reached by the rescue query.
    ///
    /// Exposed for the scan-query test: Gmail ANDs the words inside
    /// `subject:(...)`, so a rescue token matches a subject containing a phrase
    /// only when all of the token's words appear in that phrase.
    pub fn phrases_unreachable_by(rescue_terms: &[&str]) -> Vec<&'static str> {
        TRANSACTION_VERBS
            .iter()
            .chain(BALANCE_TERMS.iter())
            .copied()
            .filter(|phrase| {
                !rescue_terms
                    .iter()
                    .any(|term| term.split(' ').all(|word| phrase.contains(word)))
            })
            .collect()
    }
}
