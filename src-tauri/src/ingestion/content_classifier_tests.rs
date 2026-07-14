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
