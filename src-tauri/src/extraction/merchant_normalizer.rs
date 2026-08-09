//! Doc 30 TASK-TXN-007: Merchant Normalization Pipeline.
//!
//! Cleans a raw merchant string (uppercase, strip noise tokens), then
//! resolves it to a canonical merchant name via exact alias match, fuzzy
//! match against known merchants (>= 0.92 similarity), or auto-creates a
//! new merchant + alias if neither matches.

use anyhow::Result;
use chrono::Utc;
use deadpool_sqlite::Pool;
use regex::Regex;
use rusqlite::Connection;
use std::sync::OnceLock;
use uuid::Uuid;

use crate::db::merchants::{self, MerchantAliasesRow, MerchantsRow};

/// Doc 30: exact alias/fuzzy match must beat this similarity score, high
/// enough to avoid incorrectly merging distinct merchants (e.g. "SWIGGY" vs
/// "SWIGGYINSTAMART" must never collapse into one entity).
const FUZZY_MATCH_THRESHOLD: f64 = 0.92;

static TRAILING_DIGITS_RE: OnceLock<Regex> = OnceLock::new();
static WHITESPACE_RE: OnceLock<Regex> = OnceLock::new();

/// Doc 30: "Uppercase and strip noise tokens (transaction-reference
/// suffixes, POS terminal codes, city/location suffixes like `*BANGALORE`,
/// trailing numeric codes)."
///
/// Real bank statement merchant strings consistently use `*` as the
/// separator between the merchant's own name and everything noisy appended
/// after it (POS terminal ID, city, order reference) — e.g.
/// `AMAZON PAY*ORDER4821`, `SWIGGY*BANGALORE`, `UBER *TRIP HELP.UBER.COM` —
/// so stripping from the first `*` onward covers both named noise
/// categories in one rule. A separate pass strips a trailing numeric code
/// for merchants that append one without a `*` separator at all.
pub fn strip_noise_tokens(merchant_raw: &str) -> String {
    let upper = merchant_raw.to_uppercase();

    let before_star = split_on_aggregator_star(&upper);

    let trailing_digits_re =
        TRAILING_DIGITS_RE.get_or_init(|| Regex::new(r"\s*\d{4,}\s*$").unwrap());
    let no_trailing_digits = trailing_digits_re.replace(before_star, "");

    let whitespace_re = WHITESPACE_RE.get_or_init(|| Regex::new(r"\s+").unwrap());
    whitespace_re
        .replace_all(no_trailing_digits.trim(), " ")
        .trim()
        .to_string()
}

/// A never-before-seen raw string is only trusted enough to become a
/// permanent `merchants` row if it looks like a brand name, not a fragment
/// of sentence boilerplate lifted from the wrong part of an email body
/// (e.g. SBI's "Dear Cardholder, This is to inform you that, Rs.245.43
/// spent..." mis-anchoring on "inform you that" instead of the merchant
/// after "at"). A genuine post-`strip_noise_tokens` merchant string is a
/// proper-noun/brand token that essentially never collides with common
/// English function words, so reject any candidate where half or more of
/// its tokens are such stopwords.
///
/// ponytail: naive stopword-fraction heuristic, not real NLP/NER. It now
/// shares one vocabulary with the extraction-time gate (see below), but it
/// still can't tell a truncated real brand ("RAZ", "CAS", "ING") from a real
/// three-letter one. Issue #12's user-triggered LLM merchant pass is the
/// intended upgrade path for that residue.
///
/// The word list lives in [`crate::extraction::lexicon::MERCHANT_STOPWORDS`],
/// shared with [`is_stopword_only_merchant`]. These two predicates apply
/// deliberately different *thresholds* to the same vocabulary -- the
/// extraction-time gate rejects only an all-stopword candidate (it runs
/// before better layers get their turn, so it must be conservative), while
/// this one rejects at half, because it guards the far more damaging
/// "auto-create a permanent merchants row" step. Previously each kept its own
/// hardcoded copy of the list, and they had drifted: this one lacked
/// "block"/"call"/"customer", that one lacked "spent"/"debited"/"credited",
/// and neither had any banking nouns at all.
pub fn is_plausible_merchant_name(cleaned: &str) -> bool {
    let tokens: Vec<&str> = cleaned.split_whitespace().collect();
    if tokens.is_empty() {
        return false;
    }

    // Shape rule, not vocabulary -- see `MIN_MERCHANT_NAME_LEN`.
    if cleaned
        .trim()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .count()
        < crate::extraction::lexicon::MIN_MERCHANT_NAME_LEN
    {
        return false;
    }

    let stopword_count = tokens
        .iter()
        .filter(|t| {
            let cleaned_tok = t.trim_matches(|c: char| !c.is_alphanumeric());
            crate::extraction::lexicon::MERCHANT_STOPWORDS
                .contains(&cleaned_tok.to_lowercase().as_str())
        })
        .count();
    stopword_count * 2 < tokens.len()
}

/// Payment gateways/aggregators that prefix, rather than follow, the real
/// merchant in a card descriptor. Uppercase; matched against the token
/// immediately before the first `*`.
///
/// `RAZ` is deliberately present alongside `RAZORPAY`: banks truncate the
/// descriptor to a fixed width, so the same Razorpay charge arrives as
/// `RAZ*YULU`, `RAZORPAY*SWIGGY LIMITE`, or `RAZORPAY*SW` depending on how
/// much room was left.
const PAYMENT_AGGREGATORS: &[&str] = &[
    "PAYU",
    "PAYTM",
    "PPSL",
    "RAZ",
    "RAZORPAY",
    "CASHFREE",
    "BILLDESK",
    "CCAVENUE",
    "ICCL",
    "EASEBUZZ",
    "INFIBEAM",
    "ATOM",
    "WORLDLINE",
    "PINELABS",
    "JUSPAY",
    "PHONEPE",
    "GPAY",
    "BHARATPE",
    "MOBIKWIK",
    "INSTAMOJO",
    "STRIPE",
    "SQ",
    "SQUARE",
    "SP",
    "WWW",
];

/// Splits a `*`-separated descriptor and returns the side that holds the real
/// merchant name.
///
/// The default is the left side, which is right for the common shape where a
/// merchant appends its own noise: `AMAZON PAY*ORDER4821`, `SWIGGY*BANGALORE`.
/// But payment gateways invert it -- `PPSL*SWIGGY`, `RAZ*YULU`,
/// `PAYU*SWIGGY LIMITED` -- and blindly keeping the left side there discards
/// the actual merchant and keeps the processor. In the real corpus that
/// collapsed ~164 transactions onto six meaningless names: every Swiggy order
/// placed through Paytm's gateway became "PPSL", every Yulu ride "RAZ".
fn split_on_aggregator_star(upper: &str) -> &str {
    let Some((head, tail)) = upper.split_once('*') else {
        return upper;
    };
    let head_trimmed = head.trim();
    let tail_trimmed = tail.trim();

    // Only override when the prefix is a *known* gateway and there is an
    // actual name after it -- an unrecognised prefix keeps the original
    // left-side behaviour rather than guessing.
    let head_last_token = head_trimmed
        .split_whitespace()
        .last()
        .unwrap_or(head_trimmed);
    if PAYMENT_AGGREGATORS.contains(&head_last_token) && !tail_trimmed.is_empty() {
        // The tail may carry its own trailing noise (`SWIGGY*BANGALORE` after
        // `PAYU*`), so keep only up to the next `*`.
        return tail_trimmed.split('*').next().unwrap_or(tail_trimmed);
    }
    head_trimmed
}

/// Runs the full pipeline: clean -> exact alias match -> fuzzy match ->
/// create-new-merchant-if-none. Returns `(merchant_entity_id,
/// normalized_name)`. Synchronous over an already-open `&Connection` since
/// the real production caller (`post_processing::run_post_processing`) runs
/// deep inside a `conn.interact()` blocking closure alongside the rest of
/// the reconciliation pipeline, with no async/pool access at that point.
pub fn normalize_merchant_sync(conn: &Connection, merchant_raw: &str) -> Result<(String, String)> {
    let cleaned = strip_noise_tokens(merchant_raw);
    if cleaned.is_empty() {
        return Ok((String::new(), cleaned));
    }

    // 1. Exact alias match.
    if let Some(m) = merchants::select_by_alias(conn, &cleaned)? {
        return Ok((m.id, m.normalized_name));
    }

    // 2. Fuzzy match against existing merchants, highest score wins, must
    //    clear the threshold. Merchant counts are small enough (hundreds,
    //    not millions) for in-memory scoring to be cheap.
    let all_merchants = merchants::select_all(conn)?;

    let mut best: Option<(f64, MerchantsRow)> = None;
    for m in all_merchants {
        let score = strsim::jaro_winkler(&cleaned, &m.normalized_name);
        if score >= FUZZY_MATCH_THRESHOLD && best.as_ref().map(|(s, _)| score > *s).unwrap_or(true)
        {
            best = Some((score, m));
        }
    }

    if let Some((score, m)) = best {
        // Seed an alias so this exact raw string resolves via the fast exact
        // path next time, without needing to re-run fuzzy matching.
        let alias = MerchantAliasesRow {
            id: Uuid::new_v4().to_string(),
            merchant_entity_id: m.id.clone(),
            alias_raw: merchant_raw.to_string(),
            alias_normalized: cleaned.clone(),
            country_code: None,
            issuer_name: None,
            confidence: score,
            created_at: Some(Utc::now().naive_utc()),
        };
        let _ = merchants::insert_alias(conn, &alias);
        return Ok((m.id, m.normalized_name));
    }

    // 3. No match at all -- before trusting a brand-new name forever, reject
    //    boilerplate-shaped fragments so they can't be learned and reused
    //    (the "seeded once, wrong forever" bug: a garbage string, once
    //    auto-created here, would otherwise exact-match itself on every
    //    future occurrence). Fall through as "no merchant identified"
    //    rather than raising an error, since this is an expected outcome
    //    for noisy extraction, not a failure.
    if !is_plausible_merchant_name(&cleaned) {
        return Ok((String::new(), String::new()));
    }

    // Auto-discover a new merchant, plus an alias for faster future exact
    // matches (Doc 15 §2 principle 8's "discovered once, reused thereafter"
    // pattern, applied to merchants the same way it applies to instruments).
    let new_id = Uuid::new_v4().to_string();
    let now = Some(Utc::now().naive_utc());
    let merchant_row = MerchantsRow {
        id: new_id.clone(),
        name: merchant_raw.to_string(),
        normalized_name: cleaned.clone(),
        source: "system".to_string(),
        is_deleted: false,
        created_at: now,
        updated_at: now,
    };
    merchants::insert(conn, &merchant_row)?;

    let alias_row = MerchantAliasesRow {
        id: Uuid::new_v4().to_string(),
        merchant_entity_id: new_id.clone(),
        alias_raw: merchant_raw.to_string(),
        alias_normalized: cleaned.clone(),
        country_code: None,
        issuer_name: None,
        confidence: 1.0,
        created_at: now,
    };
    let _ = merchants::insert_alias(conn, &alias_row);

    Ok((new_id, cleaned))
}

/// Async/pool-based wrapper around [`normalize_merchant_sync`] for callers
/// that only have a `&Pool`, not an open `&Connection`. Returns just the
/// canonical `normalized_name`.
pub async fn normalize_merchant(pool: &Pool, merchant_raw: &str) -> Result<String> {
    let conn = pool.get().await?;
    let merchant_raw = merchant_raw.to_string();
    let (_, normalized_name) = conn
        .interact(move |c| normalize_merchant_sync(c, &merchant_raw))
        .await
        .map_err(|e| anyhow::anyhow!("pool interact error: {:?}", e))??;
    Ok(normalized_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_pool() -> Pool {
        let mgr = deadpool_sqlite::Manager::from_config(
            &deadpool_sqlite::Config {
                path: ":memory:".into(),
                pool: Some(deadpool_sqlite::PoolConfig::new(1)),
            },
            deadpool_sqlite::Runtime::Tokio1,
        );
        Pool::builder(mgr).build().unwrap()
    }

    async fn dummy_migrated_pool() -> Pool {
        let db_path = crate::db::test_helpers::fresh_temp_db_path();
        crate::db::migrations::run_migrations(&db_path, None)
            .await
            .unwrap();
        let mgr = deadpool_sqlite::Manager::from_config(
            &deadpool_sqlite::Config {
                path: db_path,
                pool: Some(deadpool_sqlite::PoolConfig::new(1)),
            },
            deadpool_sqlite::Runtime::Tokio1,
        );
        Pool::builder(mgr).build().unwrap()
    }

    // -----------------------------------------------------------------
    // Doc 30 TASK-TXN-007 acceptance tests
    // -----------------------------------------------------------------

    #[test]
    fn test_noise_token_stripping() {
        assert_eq!(strip_noise_tokens("Amazon Pay*Order4821"), "AMAZON PAY");
        assert_eq!(strip_noise_tokens("Swiggy*Bangalore"), "SWIGGY");
        assert_eq!(
            strip_noise_tokens("FLIPKART INTERNET 123456789"),
            "FLIPKART INTERNET"
        );
        assert_eq!(strip_noise_tokens("  netflix.com  "), "NETFLIX.COM");
        assert_eq!(strip_noise_tokens("Uber *Trip Help.Uber.Com"), "UBER");
    }

    /// Counts rows in `merchants` -- used to assert *relative* changes
    /// (created 0 vs 1 new row) since `dummy_migrated_pool()` already ships
    /// real seed data (migration `20260101000030`: Amazon, Swiggy, Uber,
    /// Starbucks, Netflix + aliases), so an absolute count of 1 would be
    /// wrong, and reusing one of those 5 names in a manually-inserted test
    /// fixture collides with the seed's `UNIQUE(normalized_name)` row.
    async fn merchant_count(pool: &Pool) -> i64 {
        let conn = pool.get().await.unwrap();
        conn.interact(|c| {
            c.query_row("SELECT COUNT(*) FROM merchants", [], |r| r.get(0))
                .unwrap()
        })
        .await
        .unwrap()
    }

    /// Doc 30 TASK-TXN-007 acceptance test. Uses the real seed data
    /// (`migration 20260101000030`'s `alias_amz2`: raw `"Amazon Pay India"`
    /// -> normalized `"AMAZON PAY INDIA"`, pointing at merchant `"AMAZON"`)
    /// rather than a hand-rolled fixture, so no setup step is needed at all.
    #[tokio::test]
    async fn test_exact_alias_match() {
        let pool = dummy_migrated_pool().await;
        let before = merchant_count(&pool).await;

        let result = normalize_merchant(&pool, "Amazon Pay India*TxnRef4821")
            .await
            .unwrap();
        assert_eq!(result, "AMAZON");
        assert_eq!(
            merchant_count(&pool).await,
            before,
            "an exact alias match must never create a new merchant row"
        );
    }

    /// Doc 30 TASK-TXN-007 acceptance test. `"NETFLIXX"` (one extra
    /// character) against the seeded `"NETFLIX"` is well above the 0.92
    /// Jaro-Winkler threshold and has no alias registered yet -- must fall
    /// through to the fuzzy path, not create a duplicate merchant.
    #[tokio::test]
    async fn test_fuzzy_match_above_threshold() {
        let pool = dummy_migrated_pool().await;
        let before = merchant_count(&pool).await;

        let result = normalize_merchant(&pool, "NETFLIXX").await.unwrap();
        assert_eq!(result, "NETFLIX");
        assert_eq!(
            merchant_count(&pool).await,
            before,
            "fuzzy match must not create a duplicate merchant"
        );
    }

    /// Doc 30 TASK-TXN-007 acceptance test. `"NETFLIX CINEMAS"` against the
    /// seeded `"NETFLIX"` is a genuinely distinct business (a hypothetical
    /// unrelated theater chain, not the streaming service) -- well below
    /// 0.92 similarity -- and must not be merged into the existing entity.
    #[tokio::test]
    async fn test_fuzzy_match_below_threshold_creates_new_merchant() {
        let pool = dummy_migrated_pool().await;
        let before = merchant_count(&pool).await;

        assert!(
            strsim::jaro_winkler("NETFLIX CINEMAS", "NETFLIX") < FUZZY_MATCH_THRESHOLD,
            "test fixture assumption broken: these two strings must NOT be \
             above the fuzzy-match threshold for this test to be meaningful"
        );

        let result = normalize_merchant(&pool, "Netflix Cinemas").await.unwrap();
        assert_eq!(result, "NETFLIX CINEMAS");
        assert_eq!(
            merchant_count(&pool).await,
            before + 1,
            "a genuinely distinct merchant must be created, not merged"
        );
    }

    #[tokio::test]
    async fn test_no_match_creates_merchant_and_alias() {
        let pool = dummy_migrated_pool().await;
        let result = normalize_merchant(&pool, "Brand New Cafe*HQ")
            .await
            .unwrap();
        assert_eq!(result, "BRAND NEW CAFE");

        let conn = pool.get().await.unwrap();
        let found = conn
            .interact(|c| merchants::select_by_alias(c, "BRAND NEW CAFE"))
            .await
            .unwrap()
            .unwrap();
        assert!(
            found.is_some(),
            "a fresh alias must resolve on the next lookup"
        );
    }

    /// Regression test for the "garbage merchant, learned once, reused
    /// forever" bug: SBI Card's mis-anchored extraction lifting "inform you
    /// that" out of "Dear Cardholder, This is to inform you that, Rs.245.43
    /// spent..." must never become a permanent merchant.
    #[tokio::test]
    async fn test_boilerplate_fragment_is_rejected_not_learned() {
        let pool = dummy_migrated_pool().await;
        let before = merchant_count(&pool).await;

        let (entity_id, normalized_name) = {
            let conn = pool.get().await.unwrap();
            conn.interact(|c| normalize_merchant_sync(c, "inform you that"))
                .await
                .unwrap()
                .unwrap()
        };
        assert_eq!(entity_id, "");
        assert_eq!(normalized_name, "");
        assert_eq!(
            merchant_count(&pool).await,
            before,
            "a boilerplate sentence fragment must never be learned as a merchant"
        );
    }

    #[test]
    fn test_is_plausible_merchant_name() {
        assert!(!is_plausible_merchant_name("INFORM YOU THAT"));
        assert!(!is_plausible_merchant_name("THAT"));
        assert!(is_plausible_merchant_name("DREAMPLUGTECHNOLOGI"));
        assert!(is_plausible_merchant_name("AMAZON PAY INDIA"));
        assert!(is_plausible_merchant_name("BRAND NEW CAFE"));
    }

    /// The anti-merchant list, checked against the actual garbage the real
    /// corpus produced (counts are occurrences across 38,269 emails).
    #[test]
    fn test_generic_fragments_from_real_corpus_are_rejected() {
        for (name, count) in [
            ("YOUR HDFC BANK RUPAY CREDIT", 334),
            ("YOUR REFERENCE BILLER NAME HDFC CREDIT", 27),
            ("USING YOUR HDFC BANK CREDIT", 1),
            ("USING YOUR", 52),
            ("YOUR POT", 139),
            ("BANKING", 18),
            ("TRANSACTION OF INR 202", 1),
            ("YOUTUBE TRANSACTION AMOUNT INR 129", 1),
            ("VPA 8127696200@PZ", 12),
            ("ZERO PROCESSING FEE", 1),
            ("EDGE CSB BANK CREDIT CARD", 22),
            // Too short to be a name -- the shape rule, not the word list.
            ("X", 1),
            ("YS", 1),
            ("NK", 12),
            ("BL", 11),
            ("IS", 42),
        ] {
            assert!(
                !is_plausible_merchant_name(name),
                "{name:?} ({count} occurrences in the real corpus) must not become a merchant"
            );
        }
    }

    /// The other half of the contract: expanding the blocklist with banking
    /// nouns must not start rejecting real merchants that merely *contain*
    /// one. Every string here was a genuine merchant in the same corpus.
    #[test]
    fn test_real_merchants_still_accepted() {
        for name in [
            "RELIANCE RETAIL LIMITE",
            "UBER INDIA SYSTEMS PRI",
            "UTTAR PRADESH STATE ROAD TRANSPORT CORPORATION",
            "TRUFFLES HOSPITALITY PVT",
            "ZEPTO MARKETPLACE PRIVATE",
            "SWIGGY FOOD BANGALORE KAIN",
            "AIRTEL PAYM",
            "WWW OLACABS COM",
            "ZOMATOLIMITED",
            "VIDYARTHI BHAVAN COUNTER 2",
            "ADITYA RAWAL", // a person is a valid counterparty for P2P
            // Contains blocklisted nouns but is not predominantly them.
            "STANDARD CHARTERED BANK",
            "AMAZON PAY INDIA",
            "PAYTM SERVICES PRIVATE LIMITED",
        ] {
            assert!(
                is_plausible_merchant_name(name),
                "{name:?} is a real merchant and must not be blocked"
            );
        }
    }

    /// Payment-gateway descriptors put the real merchant *after* the `*`.
    #[test]
    fn test_aggregator_prefix_keeps_the_real_merchant() {
        assert_eq!(strip_noise_tokens("PPSL*SWIGGY"), "SWIGGY");
        assert_eq!(strip_noise_tokens("RAZ*YULU"), "YULU");
        assert_eq!(strip_noise_tokens("Payu*Swiggy Limited"), "SWIGGY LIMITED");
        assert_eq!(
            strip_noise_tokens("Razorpay*Swiggy Limite"),
            "SWIGGY LIMITE"
        );
        assert_eq!(strip_noise_tokens("Cashfree*SW"), "SW");
        // Real bodies carry the gateway inline: "at Payu*Swiggy Food on ..."
        assert_eq!(strip_noise_tokens("Payu*Swiggy Food"), "SWIGGY FOOD");

        // The ordinary merchant-then-noise shape must be untouched.
        assert_eq!(strip_noise_tokens("AMAZON PAY*ORDER4821"), "AMAZON PAY");
        assert_eq!(strip_noise_tokens("SWIGGY*BANGALORE"), "SWIGGY");
        assert_eq!(strip_noise_tokens("UBER *TRIP HELP.UBER.COM"), "UBER");
        // An unrecognised prefix keeps the previous left-side behaviour
        // rather than guessing which side is the merchant.
        assert_eq!(strip_noise_tokens("NETFLIX*SUBSCRIPTION"), "NETFLIX");
    }

    #[tokio::test]
    async fn test_empty_pool_does_not_panic_on_unmigrated_schema() {
        // Sanity check only: dummy_pool() (unmigrated) should surface a
        // clean Err, not panic, if ever called against a schema-less DB.
        let pool = dummy_pool();
        let result = normalize_merchant(&pool, "Some Merchant").await;
        assert!(result.is_err());
    }
}
