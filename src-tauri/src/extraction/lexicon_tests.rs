use crate::extraction::lexicon::*;

fn lower_tokens(body: &str) -> Vec<String> {
    body.split_whitespace().map(|s| s.to_lowercase()).collect()
}

#[test]
fn test_single_word_keyword_match() {
    let tokens = lower_tokens("paid towards Zomato today");
    assert_eq!(
        match_label_at(&tokens, 1, MERCHANT_LABEL_STRICT),
        Some(1),
        "single-word keyword must consume exactly one token"
    );
}

#[test]
fn test_multi_word_keyword_match() {
    let tokens = lower_tokens("amount paid to Zomato today");
    assert_eq!(
        match_label_at(&tokens, 1, MERCHANT_LABEL_STRICT),
        Some(2),
        "two-word keyword must consume exactly two tokens"
    );
}

/// A trailing colon on the label (either a single-word token like "Info:"
/// or the last word of a multi-word phrase like "Merchant Name:") must not
/// prevent the match -- mirrors Layer 3's `MERCHANT_TERMINATOR` regex's
/// leading `:?`.
#[test]
fn test_trailing_colon_on_label_does_not_block_match() {
    let tokens = lower_tokens("Info: UPI/123/Zomato");
    assert_eq!(match_label_at(&tokens, 0, MERCHANT_LABEL_STRICT), Some(1));

    let tokens2 = lower_tokens("Merchant Name: RAZ*SWIGGY");
    assert_eq!(match_label_at(&tokens2, 0, MERCHANT_LABEL_STRICT), Some(2));
}

#[test]
fn test_no_match_returns_none() {
    let tokens = lower_tokens("your account balance is low");
    assert_eq!(match_label_at(&tokens, 0, MERCHANT_LABEL_STRICT), None);
}

#[test]
fn test_multi_word_keyword_out_of_bounds_does_not_panic() {
    let tokens = lower_tokens("paid");
    // "paid to" needs 2 tokens; only 1 is present.
    assert_eq!(match_label_at(&tokens, 0, MERCHANT_LABEL_STRICT), None);
}

/// Direction/merchant-label lists must stay disjoint in intent: no debit
/// verb should also appear in the credit list or vice versa (a basic sanity
/// check on the shared lexicon itself, independent of either layer).
#[test]
fn test_direction_verb_lists_are_disjoint() {
    for v in DEBIT_VERBS {
        assert!(
            !CREDIT_VERBS.contains(v),
            "{v} appears in both DEBIT_VERBS and CREDIT_VERBS"
        );
    }
}
