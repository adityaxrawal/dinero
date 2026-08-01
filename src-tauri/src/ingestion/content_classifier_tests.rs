use super::content_classifier::*;

#[test]
fn test_transaction_alert_classified_correctly() {
    assert_eq!(
        ContentClassifier::classify("You have spent Rs. 500", "Details inside"),
        ContentClass::TransactionAlert
    );
    assert_eq!(
        ContentClassifier::classify("Transaction Alert for HDFC", "Account debited"),
        ContentClass::TransactionAlert
    );
    assert_eq!(
        ContentClassifier::classify("Your account has been debited", "Rs. 1000 debited"),
        ContentClass::TransactionAlert
    );
}

#[test]
fn test_statement_email_routed_to_statement_pipeline() {
    assert_eq!(
        ContentClassifier::classify("Your Account Statement for June", "Please find attached"),
        ContentClass::StatementEmail
    );
    assert_eq!(
        ContentClassifier::classify("e-statement generated", "View your statement"),
        ContentClass::StatementEmail
    );
}

#[test]
fn test_otp_hard_rejected() {
    assert_eq!(
        ContentClassifier::classify("Your OTP is here", "123456"),
        ContentClass::Otp
    );
    assert_eq!(
        ContentClassifier::classify("One time password for login", "Do not share"),
        ContentClass::Otp
    );
}

#[test]
fn test_kyc_hard_rejected() {
    assert_eq!(
        ContentClassifier::classify("Update your KYC", "Important"),
        ContentClass::Kyc
    );
    assert_eq!(
        ContentClassifier::classify("Aadhaar linking required", "Complete your KYC"),
        ContentClass::Kyc
    );
}

#[test]
fn test_marketing_hard_rejected() {
    assert_eq!(
        ContentClassifier::classify("Exclusive cashback offer just for you", "Apply now"),
        ContentClass::Marketing
    );
}

#[test]
fn test_marketing_keyword_with_settled_transaction_still_classified_as_transaction() {
    // Doc 30 TASK-GMAIL-005: marketing keywords hard-reject only *absent* a
    // settled-transaction amount — a real debit shouldn't be swallowed just
    // because the bank's own template also mentions a cashback offer.
    assert_eq!(
        ContentClassifier::classify(
            "Cashback Offer Inside",
            "You spent Rs. 499 today and earned cashback"
        ),
        ContentClass::TransactionAlert
    );
}

#[test]
fn test_reminder_keyword_with_completed_verb_still_classified_as_transaction() {
    assert_eq!(
        ContentClassifier::classify("Payment Due Reminder", "Rs. 500 debited from your account"),
        ContentClass::TransactionAlert
    );
}

#[test]
fn test_autopay_activation_classified_as_mandate_registration() {
    // Supersedes Cluster D's original "captured as TransactionAlert"
    // decision -- dinero-docs/design-archive/specs/2026-07-18-mandate-tracking-design.md
    // §6 migrates this to the Mandate Queue instead.
    assert_eq!(
        ContentClassifier::classify(
            "AutoPay for ScribdInc: ACTIVATED",
            "Here's the summary of your successful AutoPay transaction: Transaction Amount: INR 0.00 Merchant Name: ScribdInc"
        ),
        ContentClass::MandateRegistration
    );
}

#[test]
fn test_mandate_registration_classified_correctly() {
    // tx idx 61 (real body, gmail false-negative corpus).
    assert_eq!(
        ContentClassifier::classify(
            "Registration Success: e-Mandate set at merchant using SBI Credit Card",
            "Your e-Mandate set at merchant with SBI Credit Card ending 7603 has been registered. Merchant: ScribdInc. Also, please note that you have authorised debit of INR. 0.00 from your account towards the first Trxn. against this e-Mandate."
        ),
        ContentClass::MandateRegistration
    );
}

#[test]
fn test_mandate_cancellation_classified_correctly() {
    // tx idx 62 (real body, gmail false-negative corpus).
    assert_eq!(
        ContentClassifier::classify(
            "e-mandate Cancellation on your SBI Credit Card",
            "We observe that you have cancelled your E-mandate for SiHub ID: YPCojLhIn2 on SBI Credit Card ending 7603. The below E-mandate stands cancelled: Merchant: ScribdInc"
        ),
        ContentClass::MandateCancellation
    );
}

#[test]
fn test_mandate_registration_not_swallowed_by_transaction_verb_check() {
    // The registration body contains "authorised debit of INR. 0.00" --
    // must classify as MandateRegistration, not fall through to
    // TransactionAlert via the debit-verb check.
    assert_eq!(
        ContentClassifier::classify(
            "Registration Success: e-Mandate set at merchant",
            "you have authorised debit of INR. 0.00 from your account towards the first Trxn. against this e-Mandate."
        ),
        ContentClass::MandateRegistration
    );
}

#[test]
fn test_neobank_you_paid_phrasing_classified_as_transaction() {
    // Jupiter/UPI-app confirmation templates ("Your UPI payment was
    // successful. You paid ₹300.00. Paid to <merchant>") use "you paid"
    // rather than any of the traditional-bank verbs (spent/debited/etc.) --
    // false-negative cluster B, gmail_export false-negative remediation.
    assert_eq!(
        ContentClassifier::classify(
            "UPI transaction successful",
            "You paid ₹300.00. Paid to MAX SUPER SPECIALITY HOSPITAL"
        ),
        ContentClass::TransactionAlert
    );
    assert_eq!(
        ContentClassifier::classify(
            "Your RuPay Credit Card payment was successful",
            "You paid ₹300.00. Paid to CRED TELECOM"
        ),
        ContentClass::TransactionAlert
    );
}

#[test]
fn test_bare_paid_without_you_paid_not_treated_as_transaction_verb() {
    // "you paid" is deliberately narrower than bare "paid" -- guards against
    // "not paid"/"already paid"/"please pay" noise routing to TransactionAlert.
    assert_eq!(
        ContentClassifier::classify("Reminder: your bill is not paid yet", "Please pay by 5th"),
        ContentClass::Reminder
    );
}

#[test]
fn test_other_classes() {
    assert_eq!(
        ContentClassifier::classify("Payment Due Reminder", "Pay by 5th"),
        ContentClass::Reminder
    );
    assert_eq!(
        ContentClassifier::classify("Terms and Conditions updated", "Notice"),
        ContentClass::Noise
    );
    assert_eq!(
        ContentClassifier::classify("Hello how are you", "Let's meet"),
        ContentClass::Unknown
    );
}
