//! Turns raw bank descriptors into canonical merchant names.
//!
//! Descriptors arrive padded with acquirer codes, terminal ids and reference
//! numbers. Noise stripping removes those, and the plausibility check is the
//! safety valve: if what remains does not look like a merchant name, the raw
//! value is kept rather than a confident guess substituted for it.
use anyhow::Result;
use chrono::Utc;
use deadpool_sqlite::Pool;
use regex::Regex;
use rusqlite::Connection;
use std::sync::OnceLock;
use uuid::Uuid;

use crate::db::merchants::{self, MerchantAliasesRow, MerchantsRow};

const FUZZY_MATCH_THRESHOLD: f64 = 0.92;

static TRAILING_DIGITS_RE: OnceLock<Regex> = OnceLock::new();
static WHITESPACE_RE: OnceLock<Regex> = OnceLock::new();

/// Removes acquirer codes, terminal ids and padding from a raw descriptor.
///
/// Bank descriptors carry routing detail alongside the merchant name. Stripping
/// it is what allows the same merchant, written differently by two banks, to
/// converge on one canonical entity.
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

/// ponytail: naive stopword-fraction heuristic, not real NLP/NER. It now
pub fn is_plausible_merchant_name(cleaned: &str) -> bool {
    let tokens: Vec<&str> = cleaned.split_whitespace().collect();
    if tokens.is_empty() {
        return false;
    }

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

/// Extracts the real merchant from an aggregator-prefixed descriptor.
///
/// Payment aggregators prefix their own name onto the merchant, as in
/// `RAZORPAY*ACME STORE`. Without this every such payment would be attributed to
/// the aggregator, collapsing many distinct merchants into one.
///
/// Only the token immediately before the star is tested, so a star appearing in
/// a genuine merchant name does not trigger the split. The tail is cut at any
/// further star, since some descriptors chain several segments.
fn split_on_aggregator_star(upper: &str) -> &str {
    let Some((head, tail)) = upper.split_once('*') else {
        return upper;
    };
    let head_trimmed = head.trim();
    let tail_trimmed = tail.trim();

    let head_last_token = head_trimmed
        .split_whitespace()
        .last()
        .unwrap_or(head_trimmed);
    if PAYMENT_AGGREGATORS.contains(&head_last_token) && !tail_trimmed.is_empty() {
        return tail_trimmed.split('*').next().unwrap_or(tail_trimmed);
    }
    head_trimmed
}

/// Resolves a descriptor to a canonical merchant, consulting known aliases.
///
/// Checks the alias table before attempting to normalise, so a mapping already
/// learned is reused rather than re-derived.
pub fn normalize_merchant_sync(conn: &Connection, merchant_raw: &str) -> Result<(String, String)> {
    let cleaned = strip_noise_tokens(merchant_raw);
    if cleaned.is_empty() {
        return Ok((String::new(), cleaned));
    }

    if let Some(m) = merchants::select_by_alias(conn, &cleaned)? {
        return Ok((m.id, m.normalized_name));
    }

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

    if !is_plausible_merchant_name(&cleaned) {
        return Ok((String::new(), String::new()));
    }

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

/// Async wrapper over the synchronous normaliser, for pooled callers.
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

    async fn merchant_count(pool: &Pool) -> i64 {
        let conn = pool.get().await.unwrap();
        conn.interact(|c| {
            c.query_row("SELECT COUNT(*) FROM merchants", [], |r| r.get(0))
                .unwrap()
        })
        .await
        .unwrap()
    }

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
            "ADITYA RAWAL",
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
        assert_eq!(strip_noise_tokens("Payu*Swiggy Food"), "SWIGGY FOOD");

        assert_eq!(strip_noise_tokens("AMAZON PAY*ORDER4821"), "AMAZON PAY");
        assert_eq!(strip_noise_tokens("SWIGGY*BANGALORE"), "SWIGGY");
        assert_eq!(strip_noise_tokens("UBER *TRIP HELP.UBER.COM"), "UBER");
        assert_eq!(strip_noise_tokens("NETFLIX*SUBSCRIPTION"), "NETFLIX");
    }

    #[tokio::test]
    async fn test_empty_pool_does_not_panic_on_unmigrated_schema() {
        let pool = dummy_pool();
        let result = normalize_merchant(&pool, "Some Merchant").await;
        assert!(result.is_err());
    }
}
