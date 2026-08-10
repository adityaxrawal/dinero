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
fn amount_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)(₹|rs\.?|inr)\s?[\d,]+(\.\d{1,2})?").unwrap())
}

/// Whether the content contains something shaped like a monetary amount.
fn has_amount_pattern(content: &str) -> bool {
    amount_regex().is_match(content)
}

pub const RESCUE_SUBJECT_TERMS: [&str; 12] = [
    "spent",
    "debited",
    "credited",
    "transaction alert",
    "payment of",
    "purchase of",
    "you paid",
    "account update",
    "money credited",
    "payment received",
    "upi payment",
    "available balance",
];

/// Whether the content uses transaction language -- debited, spent, credited.
///
/// Required alongside an amount, because an amount alone appears in marketing and
/// statements too. Both signals together are what distinguish a transaction.
fn has_transaction_verb(content: &str) -> bool {
    content.contains("spent")
        || content.contains("debited")
        || content.contains("credited")
        || content.contains("transaction alert")
        || content.contains("payment of")
        || content.contains("purchase of")
        || content.contains("you paid")
}

/// Whether the message announces a cancelled standing instruction.
fn is_mandate_cancellation(content: &str) -> bool {
    content.contains("mandate cancelled")
        || content.contains("mandate cancellation")
        || content.contains("e-mandate cancellation")
        || content.contains("mandate stands cancelled")
        || content.contains("autopay deactivated")
        || content.contains("autopay cancelled")
}

/// Whether the message announces a newly registered mandate.
///
/// Distinguished from a cancellation because they move a subscription in opposite
/// directions, and confusing them would leave a cancelled charge still predicted.
fn is_mandate_registration(content: &str) -> bool {
    content.contains("mandate registered")
        || content.contains("mandate set at merchant")
        || content.contains("e-mandate created")
        || content.contains("registration success")
        || content.contains("autopay activated")
        || content.contains("successful autopay transaction")
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

        let settled_transaction = has_amount_pattern(&content) && has_transaction_verb(&content);

        if !settled_transaction
            && (subject_lower.contains("offer")
                || subject_lower.contains("exclusive")
                || subject_lower.contains("apply now")
                || subject_lower.contains("pre-approved")
                || subject_lower.contains("cashback"))
        {
            return ContentClass::Marketing;
        }

        if !has_transaction_verb(&content)
            && (subject_lower.contains("payment due")
                || subject_lower.contains("due date")
                || subject_lower.contains("reminder")
                || subject_lower.contains("overdue"))
        {
            return ContentClass::Reminder;
        }

        if subject_lower.contains("spent")
            || subject_lower.contains("debited")
            || subject_lower.contains("credited")
            || subject_lower.contains("transaction alert")
            || subject_lower.contains("payment of")
            || subject_lower.contains("purchase of")
        {
            return ContentClass::TransactionAlert;
        }

        if subject_lower.contains("account update")
            || subject_lower.contains("money credited")
            || subject_lower.contains("payment received")
            || subject_lower.contains("upi payment")
            || subject_lower.contains("available balance")
        {
            return ContentClass::BalanceUpdate;
        }

        if has_transaction_verb(&content) {
            return ContentClass::TransactionAlert;
        }

        if content.contains("account update") || content.contains("available balance") {
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
}
