//! Synthesises deterministic extraction rules from successful extractions.
//!
//! How the cheap path grows to cover more banks over time. Given a value known
//! to be correct and the text it came from, this derives a regex that locates it
//! and generalises to sibling messages.
//!
//! Two guards keep a synthesised rule honest. The self-check confirms the rule
//! reproduces the value it was derived from, and the regression check confirms it
//! does not break extractions that already worked -- without which a
//! newly-learned rule could quietly degrade every message from that bank.
pub const SPAN_FIELDS: &[&str] = &[
    "merchant",
    "amount",
    "event_time",
    "reference_id",
    "balance",
    "last4",
];

/// Fields a rule sets to a fixed value rather than capturing.
///
/// Direction and currency are properties of the template itself -- a bank's debit
/// alert is always a debit -- so they are asserted, not extracted.
pub const OVERRIDE_FIELDS: &[&str] = &["direction", "currency"];

// Upper bound on a captured span. Prevents a greedy pattern from swallowing the
// rest of the message when the expected terminator is absent.
const MAX_CAPTURE: usize = 80;

// How much surrounding text anchors a synthesised pattern. Long enough to be
// distinctive within the message, short enough to survive the minor wording
// changes a bank makes without altering its template.
const ANCHOR_CHARS: usize = 24;

/// Case-insensitive substring search returning byte offsets into the original.
///
/// The length comparison guards a real hazard: lowercasing can change a string's
/// byte length for non-ASCII text, which would make offsets computed in the
/// lowercased copy invalid against the original. In that case the search falls
/// back to an exact match, where the offsets are trustworthy.
pub fn find_ignore_case(haystack: &str, needle: &str) -> Option<std::ops::Range<usize>> {
    if needle.is_empty() {
        return None;
    }
    let hay_lower = haystack.to_lowercase();
    let needle_lower = needle.to_lowercase();
    if hay_lower.len() != haystack.len() || needle_lower.len() != needle.len() {
        return haystack.find(needle).map(|s| s..s + needle.len());
    }
    hay_lower
        .find(&needle_lower)
        .map(|s| s..s + needle_lower.len())
}

/// Turns literal text into a pattern tolerant of the parts that vary.
///
/// Runs of whitespace become `\s+` and runs of digits `\d+`, so a pattern derived
/// from one message still matches siblings that differ only in their numbers and
/// spacing. Without this generalisation a synthesised rule would match exactly
/// one message and nothing else.
fn relax_literal(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_whitespace() {
            while chars.peek().is_some_and(|n| n.is_whitespace()) {
                chars.next();
            }
            out.push_str(r"\s+");
        } else if c.is_ascii_digit() {
            while chars.peek().is_some_and(|n| n.is_ascii_digit()) {
                chars.next();
            }
            out.push_str(r"\d+");
        } else {
            out.push_str(&regex::escape(&c.to_string()));
        }
    }
    out
}

/// Re-groups an integer with Indian digit separators.
///
/// The Indian system groups the last three digits and then in pairs --
/// 12,34,567 rather than 1,234,567 -- so a rule looking for a formatted amount
/// must be able to construct the form the bank actually printed.
fn indian_group(int_part: &str) -> String {
    let n = int_part.len();
    if n <= 3 {
        return int_part.to_string();
    }
    let (head, tail) = int_part.split_at(n - 3);
    let mut groups: Vec<&str> = Vec::new();
    let mut i = head.len();
    while i > 2 {
        groups.push(&head[i - 2..i]);
        i -= 2;
    }
    if i > 0 {
        groups.push(&head[..i]);
    }
    groups.reverse();
    format!("{},{}", groups.join(","), tail)
}

/// Generates the textual forms a known value might appear as.
///
/// Synthesis works backwards from a value known to be correct, but the message
/// may render it differently -- an amount could be `1200`, `1,200.00` or
/// `12,00.00`. Every plausible form is tried so the value can be located however
/// the bank chose to print it.
pub fn needle_candidates(field_name: &str, new_value: &str) -> Vec<String> {
    let v = new_value.trim();
    if v.is_empty() {
        return Vec::new();
    }
    match field_name {
        "amount" | "balance" => {
            let Ok(minor) = v.parse::<i64>() else {
                return vec![v.to_string()];
            };
            let abs = minor.abs();
            let int_part = (abs / 100).to_string();
            let decimal = format!("{}.{:02}", int_part, abs % 100);
            let grouped_int = indian_group(&int_part);
            let grouped_decimal = format!("{}.{:02}", grouped_int, abs % 100);
            let mut out = vec![grouped_decimal, decimal, grouped_int, int_part];
            out.dedup();
            out
        }
        "event_time" => {
            let date_part = v.split_whitespace().next().unwrap_or(v);
            let Ok(d) = chrono::NaiveDate::parse_from_str(date_part, "%Y-%m-%d") else {
                return vec![v.to_string()];
            };
            [
                "%d/%m/%Y",
                "%d-%m-%Y",
                "%d.%m.%Y",
                "%d/%m/%y",
                "%d-%m-%y",
                "%d.%m.%y",
                "%d-%b-%Y",
                "%d-%b-%y",
                "%d %b %Y",
                "%d %b %y",
                "%d %b, %Y",
                "%d/%b/%Y",
                "%Y-%m-%d",
            ]
            .iter()
            .map(|f| d.format(f).to_string())
            .collect()
        }
        _ => vec![v.to_string()],
    }
}

/// Builds a regex that captures the needle using its surrounding context.
///
/// Anchors on the relaxed text either side rather than the value itself, since
/// the value changes on every message and only the surrounding template is
/// stable.
pub fn synthesize_span_regex(source: &str, needle: &str) -> Option<String> {
    let span = find_ignore_case(source, needle)?;

    let prefix_start = source[..span.start]
        .char_indices()
        .rev()
        .take(ANCHOR_CHARS)
        .last()
        .map(|(i, _)| i)
        .unwrap_or(span.start);
    let suffix_end = source[span.end..]
        .char_indices()
        .take(ANCHOR_CHARS)
        .last()
        .map(|(i, c)| span.end + i + c.len_utf8())
        .unwrap_or(span.end);

    let prefix = relax_literal(&source[prefix_start..span.start]);
    let suffix = relax_literal(&source[span.end..suffix_end]);

    if prefix.is_empty() && suffix.is_empty() {
        return None;
    }

    let pattern = format!("(?is){prefix}(.{{1,{MAX_CAPTURE}}}?){suffix}");

    let re = regex::Regex::new(&pattern).ok()?;
    let recovered = re.captures(source)?.get(1)?.as_str().trim();
    if !recovered.eq_ignore_ascii_case(needle.trim()) {
        tracing::debug!(
            expected = %needle,
            recovered = %recovered,
            "rule synthesis: pattern failed its own self-check, discarding"
        );
        return None;
    }

    Some(pattern)
}

/// Synthesises a rule payload for one field from a known-correct value.
///
/// Returns None when no reliable pattern can be derived, which is the right
/// outcome: no rule is better than one that will extract the wrong value.
pub fn synthesize(field_name: &str, source: &str, new_value: &str) -> Option<serde_json::Value> {
    if OVERRIDE_FIELDS.contains(&field_name) {
        let v = new_value.trim();
        let valid = match field_name {
            "direction" => ["debit", "credit"].contains(&v.to_lowercase().as_str()),
            "currency" => v.len() == 3 && v.chars().all(|c| c.is_ascii_alphabetic()),
            _ => false,
        };
        if !valid {
            return None;
        }
        let normalized = if field_name == "currency" {
            v.to_uppercase()
        } else {
            v.to_lowercase()
        };
        return Some(serde_json::json!({ "override_value": normalized }));
    }

    if !SPAN_FIELDS.contains(&field_name) {
        return None;
    }

    for needle in needle_candidates(field_name, new_value) {
        if let Some(regex) = synthesize_span_regex(source, &needle) {
            return Some(serde_json::json!({ "regex": regex, "capture_group": 1 }));
        }
    }
    None
}

/// ponytail: unbounded map, but the keys are `field_rules` rows — bounded by
static COMPILED_RULE_REGEXES: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<String, regex::Regex>>,
> = std::sync::OnceLock::new();

/// Compiles a rule's pattern, returning None if it is invalid.
fn compiled_rule_regex(pattern: &str) -> Option<regex::Regex> {
    let cache = COMPILED_RULE_REGEXES.get_or_init(|| std::sync::Mutex::new(Default::default()));
    let mut map = cache.lock().ok()?;
    if let Some(re) = map.get(pattern) {
        return Some(re.clone());
    }
    let re = regex::Regex::new(pattern).ok()?;
    map.insert(pattern.to_string(), re.clone());
    Some(re)
}

/// Runs a rule against text and returns what it captured.
pub fn apply_payload(payload: &serde_json::Value, source: &str) -> Option<String> {
    if let Some(v) = payload.get("override_value").and_then(|v| v.as_str()) {
        return Some(v.to_string());
    }
    let pattern = payload.get("regex")?.as_str()?;
    let group = payload
        .get("capture_group")
        .and_then(|g| g.as_u64())
        .unwrap_or(1) as usize;
    let re = compiled_rule_regex(pattern)?;
    re.captures(source)?
        .get(group)
        .map(|m| m.as_str().to_string())
}

/// Verifies a fresh rule reproduces the value it was derived from.
///
/// The first of two guards. A rule that cannot re-extract its own source value is
/// broken by construction and must never reach the live set.
pub fn self_check(payload: &serde_json::Value, source: &str, expected_needles: &[String]) -> bool {
    let Some(recovered) = apply_payload(payload, source) else {
        return false;
    };
    let recovered = recovered.trim();
    expected_needles
        .iter()
        .any(|n| recovered.eq_ignore_ascii_case(n.trim()))
}

/// Verifies a new rule does not break extractions that already worked.
///
/// The second guard, and the more important one. Learning is only safe if it
/// cannot regress: a rule that improves one message while silently corrupting a
/// hundred others is a net loss, and this is what catches that before it goes
/// live.
pub fn regression_check(
    payload: &serde_json::Value,
    samples: &[(String, Option<String>)],
    field_name: &str,
) -> Result<(), String> {
    for (body, accepted) in samples {
        let Some(accepted) = accepted.as_deref() else {
            continue;
        };
        let Some(captured) = apply_payload(payload, body) else {
            continue;
        };
        if !values_agree(field_name, captured.trim(), accepted.trim()) {
            return Err(format!(
                "would change a settled {field_name}: history has {accepted:?}, \
                 the candidate extracts {:?}",
                captured.trim()
            ));
        }
    }
    Ok(())
}

/// Compares a captured value against an accepted one, per field semantics.
///
/// Equality is field-dependent: amounts must agree numerically rather than as
/// strings, since `1,200.00` and `1200` are the same amount written differently.
fn values_agree(field_name: &str, captured: &str, accepted: &str) -> bool {
    if captured.eq_ignore_ascii_case(accepted) {
        return true;
    }
    match field_name {
        "amount" | "balance" | "event_time" => needle_candidates(field_name, accepted)
            .iter()
            .any(|n| captured.eq_ignore_ascii_case(n.trim())),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SBI_BODY: &str = "Dear Cardholder, Rs.245.43 spent on your SBI Credit Card \
                            ending 7603 at RAZ*SWIGGY LIMITE BANGALORE on 01/07/26. \
                            Not you? Call 18001234.";

    #[test]
    fn cached_rule_regexes_stay_distinct_and_repeatable() {
        let merchant = serde_json::json!({ "regex": r"at\s+(\S+)", "capture_group": 1 });
        let last4 = serde_json::json!({ "regex": r"ending\s+(\d+)", "capture_group": 1 });

        let first = apply_payload(&merchant, SBI_BODY);
        assert_eq!(first.as_deref(), Some("RAZ*SWIGGY"));
        assert_eq!(apply_payload(&merchant, SBI_BODY), first);

        assert_eq!(apply_payload(&last4, SBI_BODY).as_deref(), Some("7603"));
        assert_eq!(apply_payload(&merchant, SBI_BODY), first);

        let broken = serde_json::json!({ "regex": r"(unclosed", "capture_group": 1 });
        assert_eq!(apply_payload(&broken, SBI_BODY), None);
        assert_eq!(apply_payload(&merchant, SBI_BODY), first);

        let override_rule = serde_json::json!({ "override_value": "Swiggy" });
        assert_eq!(
            apply_payload(&override_rule, SBI_BODY).as_deref(),
            Some("Swiggy")
        );
    }

    #[test]
    fn relax_literal_collapses_digits_and_whitespace() {
        assert_eq!(relax_literal("ending 7603 at"), r"ending\s+\d+\s+at");
    }

    #[test]
    fn relax_literal_escapes_metacharacters() {
        let out = relax_literal("a*b.c");
        assert!(regex::Regex::new(&out).unwrap().is_match("a*b.c"));
        assert!(!regex::Regex::new(&out).unwrap().is_match("axbxc"));
    }

    #[test]
    fn synthesized_regex_generalises_to_the_next_email() {
        let pattern = synthesize_span_regex(SBI_BODY, "RAZ*SWIGGY LIMITE BANGALORE")
            .expect("must synthesize");
        let re = regex::Regex::new(&pattern).unwrap();
        let next = "Dear Cardholder, Rs.1,020.00 spent on your SBI Credit Card \
                    ending 4412 at RAZ*YULU BIKES on 14/07/26. Not you? Call 18009999.";
        let caps = re
            .captures(next)
            .expect("the learned rule must fire on the next email");
        assert_eq!(caps.get(1).unwrap().as_str().trim(), "RAZ*YULU BIKES");
    }

    #[test]
    fn synthesized_regex_survives_reflowed_whitespace() {
        let pattern = synthesize_span_regex(SBI_BODY, "RAZ*SWIGGY LIMITE BANGALORE").unwrap();
        let re = regex::Regex::new(&pattern).unwrap();
        assert!(re.captures(&SBI_BODY.replace(' ', "\n\n  ")).is_some());
    }

    #[test]
    fn merchant_lands_in_capture_group_one() {
        let pattern = synthesize_span_regex(SBI_BODY, "RAZ*SWIGGY LIMITE BANGALORE").unwrap();
        let re = regex::Regex::new(&pattern).unwrap();
        assert_eq!(
            re.captures(SBI_BODY)
                .unwrap()
                .get(1)
                .unwrap()
                .as_str()
                .trim(),
            "RAZ*SWIGGY LIMITE BANGALORE"
        );
    }

    #[test]
    fn synthesis_refuses_a_value_absent_from_the_source() {
        assert!(synthesize_span_regex(SBI_BODY, "ZOMATO").is_none());
        assert!(
            synthesize("merchant", SBI_BODY, "ZOMATO").is_none(),
            "a corrected value the source never contains has no anchorable span"
        );
    }

    #[test]
    fn synthesizes_an_amount_rule_from_minor_units() {
        let payload = synthesize("amount", SBI_BODY, "24543").expect("must synthesize");
        let recovered = apply_payload(&payload, SBI_BODY).unwrap();
        assert_eq!(recovered.trim(), "245.43");
    }

    #[test]
    fn synthesizes_a_date_rule_from_an_iso_date() {
        let payload =
            synthesize("event_time", SBI_BODY, "2026-07-01 00:00:00").expect("must synthesize");
        let recovered = apply_payload(&payload, SBI_BODY).unwrap();
        assert_eq!(recovered.trim(), "01/07/26");
    }

    #[test]
    fn synthesizes_a_reference_id_rule() {
        let body = "UPI txn of Rs 500 to Zomato. Ref 123456789012 on 02/07/26.";
        let payload = synthesize("reference_id", body, "123456789012").unwrap();
        assert_eq!(
            apply_payload(&payload, body).unwrap().trim(),
            "123456789012"
        );
    }

    #[test]
    fn amount_needles_cover_indian_grouping() {
        let needles = needle_candidates("amount", "102000");
        assert!(needles.contains(&"1,020.00".to_string()), "got {needles:?}");
        assert!(needles.contains(&"1020.00".to_string()), "got {needles:?}");
        assert!(needles.contains(&"1,020".to_string()), "got {needles:?}");
    }

    #[test]
    fn amount_needles_handle_lakh_grouping() {
        let needles = needle_candidates("amount", "123456700");
        assert!(
            needles.contains(&"12,34,567.00".to_string()),
            "got {needles:?}"
        );
    }

    #[test]
    fn date_needles_cover_the_formats_banks_print() {
        let needles = needle_candidates("event_time", "2026-07-01 13:45:00");
        for expected in [
            "01/07/2026",
            "01/07/26",
            "01-07-2026",
            "01-Jul-2026",
            "01 Jul 2026",
        ] {
            assert!(
                needles.contains(&expected.to_string()),
                "missing {expected} in {needles:?}"
            );
        }
    }

    #[test]
    fn direction_synthesizes_an_override_not_a_regex() {
        let payload = synthesize("direction", SBI_BODY, "credit").expect("must synthesize");
        assert_eq!(payload["override_value"], "credit");
        assert!(payload.get("regex").is_none());
        assert_eq!(
            apply_payload(&payload, "any body at all").unwrap(),
            "credit"
        );
    }

    #[test]
    fn an_override_is_rejected_for_a_value_outside_the_known_set() {
        assert!(
            synthesize("direction", SBI_BODY, "sideways").is_none(),
            "direction is a closed vocabulary; an unknown value must not become a rule"
        );
        assert!(synthesize("currency", SBI_BODY, "not-a-code").is_none());
    }

    #[test]
    fn self_check_rejects_a_pattern_that_recovers_the_wrong_span() {
        let payload = serde_json::json!({"regex": r"(?is)Rs\.(.{1,80}?)\s", "capture_group": 1});
        assert!(
            !self_check(
                &payload,
                SBI_BODY,
                &["RAZ*SWIGGY LIMITE BANGALORE".to_string()]
            ),
            "a pattern that captures something else must not pass"
        );
    }

    #[test]
    fn self_check_rejects_an_uncompilable_pattern() {
        let payload = serde_json::json!({"regex": "unclosed(", "capture_group": 1});
        assert!(!self_check(&payload, SBI_BODY, &["anything".to_string()]));
    }

    #[test]
    fn self_check_accepts_the_pattern_it_was_built_from() {
        let payload = synthesize("merchant", SBI_BODY, "RAZ*SWIGGY LIMITE BANGALORE").unwrap();
        assert!(self_check(
            &payload,
            SBI_BODY,
            &needle_candidates("merchant", "RAZ*SWIGGY LIMITE BANGALORE")
        ));
    }

    #[test]
    fn an_unknown_field_name_never_synthesizes() {
        assert!(synthesize("notes", SBI_BODY, "anything").is_none());
        assert!(synthesize("category_id", SBI_BODY, "cat_1").is_none());
    }

    #[test]
    fn regression_check_passes_when_no_samples_exist() {
        let payload = serde_json::json!({"regex": "at (.+) on", "capture_group": 1});
        assert!(
            regression_check(&payload, &[], "merchant").is_ok(),
            "a new bank with no history must not be blocked from learning"
        );
    }

    #[test]
    fn regression_check_passes_when_the_rule_agrees_with_history() {
        let payload = serde_json::json!({"regex": r"at (.+?) on", "capture_group": 1});
        let samples = vec![
            (
                "Rs 100 at Amazon on 01/07/26".to_string(),
                Some("Amazon".to_string()),
            ),
            (
                "Rs 200 at Swiggy on 02/07/26".to_string(),
                Some("Swiggy".to_string()),
            ),
        ];
        assert!(regression_check(&payload, &samples, "merchant").is_ok());
    }

    #[test]
    fn regression_check_rejects_a_rule_that_rewrites_a_settled_answer() {
        let payload = serde_json::json!({"regex": r"Rs (\d+) at", "capture_group": 1});
        let samples = vec![(
            "Rs 100 at Amazon on 01/07/26".to_string(),
            Some("Amazon".to_string()),
        )];
        let err = regression_check(&payload, &samples, "merchant")
            .expect_err("a rule that changes an accepted answer must be rejected");
        assert!(
            err.contains("Amazon"),
            "the rejection must name what it would have broken: {err}"
        );
    }

    #[test]
    fn regression_check_ignores_samples_the_rule_does_not_match() {
        let payload = serde_json::json!({"regex": r"spent at (.+?) using", "capture_group": 1});
        let samples = vec![(
            "Rs 100 at Amazon on 01/07/26".to_string(),
            Some("Amazon".to_string()),
        )];
        assert!(regression_check(&payload, &samples, "merchant").is_ok());
    }

    #[test]
    fn regression_check_ignores_samples_with_no_settled_answer() {
        let payload = serde_json::json!({"regex": r"at (.+?) on", "capture_group": 1});
        let samples = vec![("Rs 100 at Amazon on 01/07/26".to_string(), None)];
        assert!(
            regression_check(&payload, &samples, "merchant").is_ok(),
            "a sample with no accepted value cannot be regressed against"
        );
    }

    #[test]
    fn regression_check_compares_amounts_in_minor_units() {
        let payload = serde_json::json!({"regex": r"Rs\.([\d.]+) ", "capture_group": 1});
        let samples = vec![(
            "Rs.245.43 spent today".to_string(),
            Some("24543".to_string()),
        )];
        assert!(
            regression_check(&payload, &samples, "amount").is_ok(),
            "the comparison must normalise printed money to stored minor units"
        );
    }

    #[test]
    fn regression_check_catches_a_genuinely_wrong_amount() {
        let payload = serde_json::json!({"regex": r"ending (\d+) ", "capture_group": 1});
        let samples = vec![(
            "Rs.245.43 spent ending 7603 today".to_string(),
            Some("24543".to_string()),
        )];
        assert!(regression_check(&payload, &samples, "amount").is_err());
    }

    #[test]
    fn regression_check_compares_dates_after_normalising_format() {
        let payload = serde_json::json!({"regex": r"on (\d{2}/\d{2}/\d{2})", "capture_group": 1});
        let samples = vec![(
            "Spent on 01/07/26 today".to_string(),
            Some("2026-07-01 00:00:00".to_string()),
        )];
        assert!(
            regression_check(&payload, &samples, "event_time").is_ok(),
            "01/07/26 and 2026-07-01 are the same date"
        );
    }
}
