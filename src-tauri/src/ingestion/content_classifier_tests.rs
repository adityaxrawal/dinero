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
fn test_balance_subject_does_not_swallow_a_real_debit() {
    // BalanceUpdate overwrites amount_minor with 0 downstream
    // (message_processor::apply_balance_update_placeholder), so a debit whose
    // subject only mentions the balance must still reach TransactionAlert.
    assert_eq!(
        ContentClassifier::classify(
            "Available balance update on your A/c XX7603",
            "Rs. 500 debited. Available balance: Rs. 12,340.00"
        ),
        ContentClass::TransactionAlert
    );
}

#[test]
fn test_future_debit_notice_is_not_a_settled_transaction() {
    // Pre-debit mandate notices quote an amount and the word "debited"; acting
    // on them would book a charge that has not happened yet.
    assert_eq!(
        ContentClassifier::classify(
            "Upcoming payment reminder",
            "Rs. 199.00 will be debited from your account on 05-Sep towards Netflix."
        ),
        ContentClass::Reminder
    );
}

#[test]
fn test_warning_and_real_charge_in_one_mail_keeps_the_real_charge() {
    // Stripping future forms must be per-phrase, not a whole-message veto.
    assert_eq!(
        ContentClassifier::classify(
            "Transaction on your card",
            "Rs. 750 debited at BIGBASKET. Rs. 199 will be debited on 05-Sep."
        ),
        ContentClass::TransactionAlert
    );
}

#[test]
fn test_failed_transaction_is_not_booked() {
    assert_eq!(
        ContentClassifier::classify(
            "Payment update",
            "Your payment of Rs. 2,500 to AMAZON failed due to insufficient balance."
        ),
        ContentClass::Noise
    );
}

#[test]
fn test_expanded_verbs_reach_transaction_alert() {
    for (subject, body) in [
        (
            "ATM cash withdrawal",
            "Rs. 5,000 withdrawn from ATM at Andheri",
        ),
        (
            "Refund processed",
            "Refund of Rs. 899 for order 123 has been processed",
        ),
        ("Salary", "INR. 85,000.00 deposited to your account"),
        (
            "Card transaction",
            "Your card was used at SWIGGY for Rs 420",
        ),
        (
            "Fund transfer",
            "Transfer of Rs 1,200 transferred to RAHUL K",
        ),
        ("Netbanking", "Amount received: 0.00 INR from ACME LTD"),
    ] {
        assert_eq!(
            ContentClassifier::classify(subject, body),
            ContentClass::TransactionAlert,
            "missed transaction phrasing in {body:?}"
        );
    }
}

#[test]
fn test_negated_verbs_do_not_fabricate_a_transaction() {
    assert_eq!(
        ContentClassifier::classify(
            "Important notice",
            "Rs. 500 was not debited from your account."
        ),
        ContentClass::Noise
    );
}

#[test]
fn test_expanded_mandate_phrasings() {
    assert_eq!(
        ContentClassifier::classify(
            "Standing instruction update",
            "Your standing instruction has been cancelled for Merchant: ScribdInc"
        ),
        ContentClass::MandateCancellation
    );
    assert_eq!(
        ContentClassifier::classify("AutoPay enabled", "AutoPay enabled for Merchant: Spotify"),
        ContentClass::MandateRegistration
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
