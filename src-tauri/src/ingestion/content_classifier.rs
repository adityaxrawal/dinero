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

/// Doc 30 TASK-GMAIL-005: a currency-marked amount (₹/Rs./INR followed by
/// digits) — deliberately narrower than "any digits", since precision over
/// recall (Doc 12 §6.2) means a bare number shouldn't count as a settled
/// transaction signal.
fn amount_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)(₹|rs\.?|inr)\s?[\d,]+(\.\d{1,2})?").unwrap())
}

fn has_amount_pattern(content: &str) -> bool {
    amount_regex().is_match(content)
}

/// Every subject-line phrase that can make `classify(subject, "")` return
/// `TransactionAlert` or `BalanceUpdate`.
///
/// Gate 1's "Requirement 6" rescue re-classifies on subject alone, and a hit
/// there promotes an *otherwise-rejected* sender to
/// `VerifiedTransactionCandidate("Unknown Bank")` — so a bank that isn't in
/// the bundled registry can still be picked up. A historical scan's
/// server-side `from:` prefilter would never fetch those messages, silently
/// killing that rescue, so it also issues a `subject:` query built from this
/// list.
///
/// MUST stay a superset of the phrases checked in `classify()` rules 6/6b and
/// `has_transaction_verb` — anything added there and not here becomes mail the
/// scan can no longer discover. `subject_terms_cover_classifier_rescue_phrases`
/// enforces that.
pub const RESCUE_SUBJECT_TERMS: [&str; 12] = [
    // has_transaction_verb / rule 6
    "spent",
    "debited",
    "credited",
    "transaction alert",
    "payment of",
    "purchase of",
    "you paid",
    // rule 6b (balance update)
    "account update",
    "money credited",
    "payment received",
    "upi payment",
    "available balance",
];

fn has_transaction_verb(content: &str) -> bool {
    content.contains("spent")
        || content.contains("debited")
        || content.contains("credited")
        || content.contains("transaction alert")
        || content.contains("payment of")
        || content.contains("purchase of")
        // Neobank/UPI-app confirmation phrasing (e.g. Jupiter: "You paid
        // ₹300.00. Paid to <merchant>") -- narrower than bare "paid" so it
        // doesn't also match "not paid"/"already paid"/"please pay" noise.
        || content.contains("you paid")
}

/// docs/superpowers/specs/2026-07-18-mandate-tracking-design.md §4.1.
/// Checked before any transaction-verb logic (see `classify()`) since
/// mandate emails legitimately contain debit-shaped language ("authorised
/// debit of INR 0.00") that must not fall through to TransactionAlert.
fn is_mandate_cancellation(content: &str) -> bool {
    content.contains("mandate cancelled")
        || content.contains("mandate cancellation")
        || content.contains("e-mandate cancellation")
        || content.contains("mandate stands cancelled")
        || content.contains("autopay deactivated")
        || content.contains("autopay cancelled")
}

/// docs/superpowers/specs/2026-07-18-mandate-tracking-design.md §4.1.
/// Supersedes Cluster D's original `has_transaction_verb` addition for
/// "successful autopay transaction" -- that phrase now routes here instead,
/// before transaction-verb logic ever runs.
fn is_mandate_registration(content: &str) -> bool {
    content.contains("mandate registered")
        || content.contains("mandate set at merchant")
        || content.contains("e-mandate created")
        || content.contains("registration success")
        || content.contains("autopay activated")
        || content.contains("successful autopay transaction")
}

impl ContentClassifier {
    pub fn classify(subject: &str, body: &str) -> ContentClass {
        let subject_lower = subject.to_lowercase();
        let body_lower = body.to_lowercase();

        let content = format!("{} {}", subject_lower, body_lower);

        // 1. Check OTP — hard-reject regardless of any amount-like pattern present.
        if subject_lower.contains("otp")
            || subject_lower.contains("one time password")
            || subject_lower.contains("verification code")
        {
            return ContentClass::Otp;
        }

        // 2. Check KYC — hard-reject regardless of any amount-like pattern present.
        if subject_lower.contains("kyc")
            || subject_lower.contains("know your customer")
            || subject_lower.contains("pan update")
            || subject_lower.contains("aadhaar")
        {
            return ContentClass::Kyc;
        }

        // 3. Statement — takes priority over marketing/reminder/transaction
        // classification (Doc 12 §6.2: a transaction candidate must be "not
        // a statement email").
        if subject_lower.contains("statement") || subject_lower.contains("e-statement") {
            return ContentClass::StatementEmail;
        }

        // 3b. Mandate lifecycle events -- checked before any transaction-verb
        // logic (docs/superpowers/specs/2026-07-18-mandate-tracking-design.md
        // §4.1). Cancellation checked before registration since some
        // wording could plausibly overlap ("mandate ... cancelled" contains
        // no registration phrase, but keeping cancellation first is the
        // more conservative order if that ever changes).
        if is_mandate_cancellation(&content) {
            return ContentClass::MandateCancellation;
        }
        if is_mandate_registration(&content) {
            return ContentClass::MandateRegistration;
        }

        // 3c. Loan/credit-line ads unconditionally -- unlike a cashback line
        // tacked onto a real transaction receipt, this subject phrasing is
        // never used for an actual settled transaction, so it must not be
        // overridden by the `settled_transaction` bypass below (loan-ad copy
        // routinely uses transaction verbs like "credited" to describe
        // disbursal speed, e.g. "Money credited in 2 minutes").
        if subject_lower.contains("personal loan") || subject_lower.contains("loan offer") {
            return ContentClass::Marketing;
        }

        // A settled-transaction signal — computed once, used to keep a real
        // transaction from being swallowed by an incidental marketing/reminder
        // keyword (e.g. "Cashback Offer: You spent Rs. 499 today").
        let settled_transaction = has_amount_pattern(&content) && has_transaction_verb(&content);

        // 4. Marketing — hard-reject only absent a settled-transaction amount.
        if !settled_transaction
            && (subject_lower.contains("offer")
                || subject_lower.contains("exclusive")
                || subject_lower.contains("apply now")
                || subject_lower.contains("pre-approved")
                || subject_lower.contains("cashback"))
        {
            return ContentClass::Marketing;
        }

        // 5. Reminder — routes separately only absent a completed-transaction verb.
        if !has_transaction_verb(&content)
            && (subject_lower.contains("payment due")
                || subject_lower.contains("due date")
                || subject_lower.contains("reminder")
                || subject_lower.contains("overdue"))
        {
            return ContentClass::Reminder;
        }

        // 6. Transaction Alert
        if subject_lower.contains("spent")
            || subject_lower.contains("debited")
            || subject_lower.contains("credited")
            || subject_lower.contains("transaction alert")
            || subject_lower.contains("payment of")
            || subject_lower.contains("purchase of")
        {
            return ContentClass::TransactionAlert;
        }

        // 6b. Balance Update (often missed as just 'account update' or 'upi payment' with no exact amount in subject).
        //
        // Deliberately NOT gated on `has_amount_pattern` -- these subject
        // phrases double as `RESCUE_SUBJECT_TERMS`, matched against a bare
        // subject with no body yet fetched (the historical scan's
        // server-side `subject:` prefilter query), so the guard would also
        // reject those and silently break discovery. This does mean some
        // pure marketing/login notices reusing the same generic subject
        // wording (e.g. HDFC's "Account update for your HDFC Bank A/c" sent
        // for a T&Cs acceptance, not a real balance change) still get routed
        // here -- Layer 6 finding nothing extractable and the
        // `Layer6Outcome::Rejected` terminal state (`process_layer6_job`)
        // are the backstop for that, not this classifier.
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

        // 7. Noise
        if subject_lower.contains("terms")
            || subject_lower.contains("conditions")
            || subject_lower.contains("important notice")
        {
            return ContentClass::Noise;
        }

        ContentClass::Unknown
    }
}
