use regex::Regex;
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentClass {
    TransactionAlert,
    BalanceUpdate,
    StatementEmail,
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

fn has_transaction_verb(content: &str) -> bool {
    content.contains("spent")
        || content.contains("debited")
        || content.contains("credited")
        || content.contains("transaction alert")
        || content.contains("payment of")
        || content.contains("purchase of")
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

        // 6b. Balance Update (often missed as just 'account update' or 'upi payment' with no exact amount in subject)
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
