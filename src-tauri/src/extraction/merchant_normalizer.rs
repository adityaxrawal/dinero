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

    let before_star = upper.split('*').next().unwrap_or(&upper);

    let trailing_digits_re =
        TRAILING_DIGITS_RE.get_or_init(|| Regex::new(r"\s*\d{4,}\s*$").unwrap());
    let no_trailing_digits = trailing_digits_re.replace(before_star, "");

    let whitespace_re = WHITESPACE_RE.get_or_init(|| Regex::new(r"\s+").unwrap());
    whitespace_re
        .replace_all(no_trailing_digits.trim(), " ")
        .trim()
        .to_string()
}

/// Runs the full pipeline: clean -> exact alias match -> fuzzy match ->
/// create-new-merchant-if-none. Returns the canonical `normalized_name` to
/// store on the observation/transaction.
pub async fn normalize_merchant(pool: &Pool, merchant_raw: &str) -> Result<String> {
    let cleaned = strip_noise_tokens(merchant_raw);
    if cleaned.is_empty() {
        return Ok(cleaned);
    }

    let conn = pool.get().await?;

    // 1. Exact alias match.
    let cleaned_for_exact = cleaned.clone();
    let exact = conn
        .interact(move |c| merchants::select_by_alias(c, &cleaned_for_exact))
        .await
        .map_err(|e| anyhow::anyhow!("pool interact error: {:?}", e))??;
    if let Some(m) = exact {
        return Ok(m.normalized_name);
    }

    // 2. Fuzzy match against existing merchants, highest score wins, must
    //    clear the threshold. A DB round trip per candidate is avoided by
    //    scoring in memory -- merchant counts are small enough (hundreds,
    //    not millions) for this to be cheap.
    let all_merchants = conn
        .interact(|c| merchants::select_all(c))
        .await
        .map_err(|e| anyhow::anyhow!("pool interact error: {:?}", e))??;

    let mut best: Option<(f64, MerchantsRow)> = None;
    for m in all_merchants {
        let score = strsim::jaro_winkler(&cleaned, &m.normalized_name);
        if score >= FUZZY_MATCH_THRESHOLD {
            if best.as_ref().map(|(s, _)| score > *s).unwrap_or(true) {
                best = Some((score, m));
            }
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
        let _ = conn
            .interact(move |c| merchants::insert_alias(c, &alias))
            .await;
        return Ok(m.normalized_name);
    }

    // 3. No match at all -- auto-discover a new merchant, plus an alias for
    //    faster future exact matches (Doc 15 §2 principle 8's "discovered
    //    once, reused thereafter" pattern, applied to merchants the same
    //    way it applies to instruments).
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
    conn.interact(move |c| merchants::insert(c, &merchant_row))
        .await
        .map_err(|e| anyhow::anyhow!("pool interact error: {:?}", e))??;

    let alias_row = MerchantAliasesRow {
        id: Uuid::new_v4().to_string(),
        merchant_entity_id: new_id,
        alias_raw: merchant_raw.to_string(),
        alias_normalized: cleaned.clone(),
        country_code: None,
        issuer_name: None,
        confidence: 1.0,
        created_at: now,
    };
    let _ = conn
        .interact(move |c| merchants::insert_alias(c, &alias_row))
        .await;

    Ok(cleaned)
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
        assert_eq!(strip_noise_tokens("FLIPKART INTERNET 123456789"), "FLIPKART INTERNET");
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

        let result = normalize_merchant(&pool, "Netflix Cinemas")
            .await
            .unwrap();
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
        let result = normalize_merchant(&pool, "Brand New Cafe*HQ").await.unwrap();
        assert_eq!(result, "BRAND NEW CAFE");

        let conn = pool.get().await.unwrap();
        let found = conn
            .interact(|c| merchants::select_by_alias(c, "BRAND NEW CAFE"))
            .await
            .unwrap()
            .unwrap();
        assert!(found.is_some(), "a fresh alias must resolve on the next lookup");
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
