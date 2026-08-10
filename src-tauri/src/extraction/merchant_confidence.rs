//! Scores how trustworthy an extracted merchant name is.
//!
//! The gate on merchant normalisation: a low-confidence name is left as the raw
//! descriptor rather than being confidently rewritten into something wrong,
//! since a bad normalisation silently corrupts spending analytics.
//! ponytail: hand-tuned additive heuristic, not a learned model. It only has

pub const LOW_CONFIDENCE_THRESHOLD: f64 = 0.60;

const UNESTABLISHED_PENALTY: f64 = 0.30;
const CODE_SHAPE_PENALTY: f64 = 0.20;
const STOPWORD_PENALTY: f64 = 0.15;

/// Starting confidence, set by which layer produced the name.
///
/// A deterministic rule match is more trustworthy than an LLM inference, so the
/// method the value came from is the strongest single signal available.
fn base_score(extraction_method: Option<&str>) -> f64 {
    match extraction_method {
        Some("learned_patterns") => 0.90,
        Some("bank_templates") => 0.85,
        Some(m) if m.starts_with("layer5") => 0.85,
        Some("llm_layer6") => 0.80,
        Some("generic_regex") => 0.60,
        Some("nlp") => 0.45,
        _ => 0.50,
    }
}

/// Whether a candidate looks like a reference code rather than a name.
///
/// Terminal ids and acquirer references survive noise stripping and would
/// otherwise be recorded as merchants, splitting one merchant's spending across
/// many meaningless entries.
fn looks_like_code(cleaned: &str) -> bool {
    let alnum: Vec<char> = cleaned.chars().filter(|c| c.is_alphanumeric()).collect();
    if alnum.len() < 5 {
        return true;
    }
    !alnum
        .iter()
        .any(|c| matches!(c.to_ascii_lowercase(), 'a' | 'e' | 'i' | 'o' | 'u'))
}

/// Whether a candidate still contains filler words after cleaning.
fn has_stopword_token(cleaned: &str) -> bool {
    cleaned.split_whitespace().any(|t| {
        let tok = t.trim_matches(|c: char| !c.is_alphanumeric());
        crate::extraction::lexicon::MERCHANT_STOPWORDS.contains(&tok.to_lowercase().as_str())
    })
}

/// Scores how much to trust an extracted merchant name.
///
/// The gate on normalisation: a low score means the raw descriptor is kept rather
/// than confidently rewritten into something wrong. A bad normalisation is worse
/// than an ugly name, because it silently corrupts spending analytics while
/// looking correct.
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

    #[test]
    fn corpus_garbage_scores_below_corpus_real_merchants() {
        let garbage = [
            ("nlp", "YOUR POT", false),
            ("generic_regex", "USING YOUR", false),
            ("generic_regex", "RAZ", false),
            ("nlp", "BANKING", false),
            ("generic_regex", "NK", false),
            ("bank_templates", "YOUR HDFC BANK RUPAY CREDIT", false),
        ];
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
