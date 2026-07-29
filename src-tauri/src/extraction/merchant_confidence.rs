//! Issue #12: a per-merchant confidence score, used to decide which
//! transactions the user-triggered "Normalize with LLM" pass should spend
//! inference on.
//!
//! Why this is computed on read rather than stored at extraction time:
//! `ExtractionResult::confidence_score` is a *whole-extraction* number and,
//! by its own doc comment, most layers deliberately leave it `None` --
//! including Layer 2, which produces the majority of extractions. Writing a
//! new `merchant_confidence` column at extraction time would therefore leave
//! every already-scanned transaction NULL, and the cleanup pass exists
//! precisely to fix the backlog the user already has. Deriving the score from
//! data that is *already* persisted (which layer ran, whether the name
//! resolved to an established merchant, and the shape of the name itself)
//! makes it work on the existing database with no migration backfill and no
//! rescan.
//!
//! ponytail: hand-tuned additive heuristic, not a learned model. It only has
//! to rank badly-extracted merchants above cleanly-extracted ones well enough
//! to pick a work queue -- the LLM pass is what actually decides correctness,
//! and a wrong ranking costs one wasted inference, not a wrong transaction.

/// Merchants scoring below this are offered to the LLM cleanup pass.
///
/// Calibrated against real corpus strings (see the tests): every measured
/// garbage capture lands at or below 0.40, while a correctly-extracted name
/// resolving to an established merchant bottoms out at 0.60.
///
/// Be aware what this means on a *fresh* database. Replaying the 38k-email
/// corpus, all 528 distinct merchants that reach a `merchants` row score
/// below this, because none is established yet — nothing has corroborated
/// any of them. That is not a mis-calibration to tune away:
///
/// * No heuristic can separate "SWIGGY LIMITE" (a truncation needing repair)
///   from "ZOOMCAR INDIA PVT LTD" (already correct). Both are clean-looking
///   names from a good layer with no corroboration. Telling them apart is
///   the entire reason the LLM is involved.
/// * The pass also assigns categories, and no transaction has one today
///   (issue #4), so every transaction has to be visited regardless.
///
/// What keeps this workable is ordering, not filtering: the queue is sorted
/// worst-first, the user sees the count before committing, and the run can be
/// cancelled at any point having already fixed the most damaged rows.
pub const LOW_CONFIDENCE_THRESHOLD: f64 = 0.60;

/// Penalty for a merchant that no established entity backs.
///
/// The single largest term, because it is the single strongest signal. Note
/// it is deliberately *not* "did this string hit a `merchant_aliases` row" --
/// `normalize_merchant_sync` writes an alias for every merchant it resolves,
/// including ones it auto-created from one noisy email, so alias existence
/// alone is true almost everywhere and carries no information. See
/// [`score_merchant`]'s `established_merchant` parameter.
const UNESTABLISHED_PENALTY: f64 = 0.30;
/// Penalty for a name shaped like a machine code rather than a brand.
const CODE_SHAPE_PENALTY: f64 = 0.20;
/// Penalty for a name carrying banking/boilerplate vocabulary.
const STOPWORD_PENALTY: f64 = 0.15;

/// Base confidence per extraction layer, ordered by how much the layer
/// actually *knew* about where the merchant lived in the text.
///
/// A learned rule and a bank template were both told which capture group is
/// the merchant, so they rank highest. `llm_layer6` is high because a
/// previous LLM pass already read the body. `generic_regex` guessed from a
/// label keyword ("at"/"to"/"from"), and `nlp` ran only after that guess
/// already failed -- which is why it is the most suspect of all.
fn base_score(extraction_method: Option<&str>) -> f64 {
    match extraction_method {
        Some("learned_patterns") => 0.90,
        Some("bank_templates") => 0.85,
        Some(m) if m.starts_with("layer5") => 0.85,
        Some("llm_layer6") => 0.80,
        Some("generic_regex") => 0.60,
        Some("nlp") => 0.45,
        // Includes `pending_llm_enrichment` and any method added later: an
        // unrecognised source is not evidence of quality either way, so it
        // sits below every known-good layer but above the worst one.
        _ => 0.50,
    }
}

/// `true` when the name reads as a machine code rather than a brand --
/// "RAZ", "ICCL/M", "A2AINT01", "NK". Two independent shapes catch this:
/// too short to be a brand at all, or long enough but carrying no vowel,
/// which is what abbreviations and terminal codes look like.
fn looks_like_code(cleaned: &str) -> bool {
    let alnum: Vec<char> = cleaned.chars().filter(|c| c.is_alphanumeric()).collect();
    if alnum.len() < 5 {
        return true;
    }
    !alnum
        .iter()
        .any(|c| matches!(c.to_ascii_lowercase(), 'a' | 'e' | 'i' | 'o' | 'u'))
}

/// `true` when any token is banking/boilerplate vocabulary. Unlike
/// [`crate::extraction::merchant_normalizer::is_plausible_merchant_name`],
/// which rejects only at *half* the tokens, a single such token is enough to
/// dent confidence here -- "SWIGGY LIMITE" is fine, but "HDFC BANK CREDIT"
/// and "YOUR POT" both carry one and are both worth a second look. This is a
/// ranking signal, not a rejection.
fn has_stopword_token(cleaned: &str) -> bool {
    cleaned.split_whitespace().any(|t| {
        let tok = t.trim_matches(|c: char| !c.is_alphanumeric());
        crate::extraction::lexicon::MERCHANT_STOPWORDS.contains(&tok.to_lowercase().as_str())
    })
}

/// Scores one merchant extraction in `[0.0, 1.0]`.
///
/// `established_merchant` means the resolved `merchants` row has independent
/// corroboration: either the user vouched for it (`source = 'user'`) or it
/// carries more than one alias, i.e. at least two differently-spelled raw
/// strings have been tied to it. An auto-created merchant has exactly one
/// alias -- itself -- and so fails this test, which is the point: that is
/// exactly the "seeded once from a bad extraction, reused forever" case the
/// LLM pass is meant to unwind.
pub fn score_merchant(
    extraction_method: Option<&str>,
    cleaned_name: &str,
    established_merchant: bool,
) -> f64 {
    if cleaned_name.trim().is_empty() {
        return 0.0;
    }

    let mut score = base_score(extraction_method);
    if !established_merchant {
        score -= UNESTABLISHED_PENALTY;
    }
    if looks_like_code(cleaned_name) {
        score -= CODE_SHAPE_PENALTY;
    }
    if has_stopword_token(cleaned_name) {
        score -= STOPWORD_PENALTY;
    }
    score.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ranking this whole feature depends on: the real garbage the corpus
    /// produces must sort below the real merchants it produces. If this
    /// inverts, the cleanup pass spends its inference on the wrong rows.
    #[test]
    fn corpus_garbage_scores_below_corpus_real_merchants() {
        // Left the ladder as bad captures -- every one of these is a real
        // string measured in the 38k-email corpus, and none of them would
        // ever accumulate a second alias.
        let garbage = [
            ("nlp", "YOUR POT", false),
            ("generic_regex", "USING YOUR", false),
            ("generic_regex", "RAZ", false),
            ("nlp", "BANKING", false),
            ("generic_regex", "NK", false),
            ("bank_templates", "YOUR HDFC BANK RUPAY CREDIT", false),
        ];
        // Genuinely correct extractions resolving to established merchants.
        let good = [
            ("bank_templates", "SWIGGY", true),
            ("bank_templates", "AMAZON PAY INDIA", true),
            ("learned_patterns", "UBER INDIA SYSTEMS", true),
            ("generic_regex", "STARBUCKS COFFEE", true),
        ];

        let worst_good = good
            .iter()
            .map(|(m, n, a)| score_merchant(Some(m), n, *a))
            .fold(f64::MAX, f64::min);
        let best_garbage = garbage
            .iter()
            .map(|(m, n, a)| score_merchant(Some(m), n, *a))
            .fold(f64::MIN, f64::max);

        assert!(
            best_garbage < worst_good,
            "garbage must rank below real merchants: best garbage {best_garbage}, worst good {worst_good}"
        );
        assert!(
            best_garbage < LOW_CONFIDENCE_THRESHOLD,
            "every known-bad merchant must fall under the LLM threshold, got {best_garbage}"
        );
        assert!(
            worst_good >= LOW_CONFIDENCE_THRESHOLD,
            "no known-good merchant may be sent to the LLM, got {worst_good}"
        );
    }

    /// The case the LLM exists to fix: bank-truncated brands. These come from
    /// a *good* layer (the template found the right span) and are not
    /// code-shaped, so the established-merchant signal is the only thing that
    /// can catch them -- each truncation auto-creates its own orphan merchant
    /// rather than joining the real one.
    #[test]
    fn truncated_brands_from_a_good_layer_still_qualify() {
        for name in ["SWIGGY LIMITE", "SWIGGY FOOD", "WWW SWIGGY COM"] {
            let s = score_merchant(Some("bank_templates"), name, false);
            assert!(
                s < LOW_CONFIDENCE_THRESHOLD,
                "unestablished truncation {name:?} must be offered to the LLM, got {s}"
            );
        }
    }

    /// Symmetric guard on the above: once a truncation has been tied to the
    /// real merchant (the LLM pass's own output), it must stop qualifying, or
    /// every run would re-process the rows the previous run just fixed.
    #[test]
    fn a_fixed_merchant_stops_qualifying() {
        let s = score_merchant(Some("bank_templates"), "SWIGGY LIMITE", true);
        assert!(
            s >= LOW_CONFIDENCE_THRESHOLD,
            "an established merchant must not be re-queued, got {s}"
        );
    }

    #[test]
    fn code_shape_detection() {
        assert!(looks_like_code("RAZ"), "too short");
        assert!(looks_like_code("NK"), "too short");
        assert!(looks_like_code("TFR CR NFT"), "no vowel");
        assert!(!looks_like_code("SWIGGY"), "real brand");
        assert!(!looks_like_code("AMAZON PAY"), "real brand");
    }

    /// Empty is not "low confidence", it is "nothing to score" -- the caller
    /// filters these out rather than queueing them for the LLM.
    #[test]
    fn empty_name_scores_zero() {
        assert_eq!(score_merchant(Some("bank_templates"), "   ", true), 0.0);
    }

    #[test]
    fn score_never_leaves_the_unit_interval() {
        let s = score_merchant(Some("nlp"), "XX", false);
        assert!((0.0..=1.0).contains(&s), "got {s}");
    }
}
