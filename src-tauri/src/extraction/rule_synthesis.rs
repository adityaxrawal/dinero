//! Deterministic rule synthesis: turn "the answer for this field should have
//! been X" into a regex that extracts X from this source and from the next
//! source of the same shape (design 2026-07-29).
//!
//! Generalized from `merchant_llm::synthesize_merchant_regex`, which proved the
//! anchor-relax technique on merchant alone. Nothing about it was
//! merchant-specific except the assumption that the corrected value appears
//! verbatim in the source. For merchant that is true because the LLM is asked
//! for a verbatim span; for every other field it is true only after formatting
//! the stored value the way the bank prints it, which is what
//! [`needle_candidates`] exists to do — a date stored as `2026-07-01` is
//! printed `01/07/26`, and an amount stored as `102000` minor units is printed
//! `1,020.00`.
//!
//! Pure: no DB, no IO, no async. The validation gate is a property of the
//! output, not of who produced it, so an LLM-authored pattern runs through the
//! identical [`self_check`] here.

/// Fields whose value occupies a span of the source that can be anchored on.
pub const SPAN_FIELDS: &[&str] = &[
    "merchant",
    "amount",
    "event_time",
    "reference_id",
    "balance",
    "last4",
];

/// Fields that are template-level literals in a bank template
/// (`"direction": "debit"` is fixed per pattern object, not a capture group).
/// There is no span to anchor, so a correction teaches a flat override keyed to
/// the template hash instead.
pub const OVERRIDE_FIELDS: &[&str] = &["direction", "currency"];

/// Longest span the synthesized regex will capture. Generous enough for
/// "UTTAR PRADESH STATE ROAD TRANSPORT CORPORATION", short enough that a
/// mis-anchored pattern cannot swallow a paragraph.
const MAX_CAPTURE: usize = 80;

/// How much literal text on each side of the value is baked into the anchor.
/// Long enough to be specific to this template's phrasing, short enough to
/// survive the small wording differences between two alerts of the same kind.
const ANCHOR_CHARS: usize = 24;

/// Case-insensitive search returning a byte range into `haystack`.
///
/// `to_lowercase` can change byte length (e.g. 'İ'), which would make an index
/// into the lowered string invalid for the original — fall back to a
/// case-sensitive search rather than slice at a wrong offset. These sources are
/// ASCII in practice.
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

/// Turns a literal chunk of source text into a regex fragment that still
/// matches the *next* source of the same kind.
///
/// Two relaxations, both load-bearing: whitespace runs become `\s+` because
/// HTML flattening produces wildly different blank-line counts between two
/// renderings of one template; digit runs become `\d+` because the anchor
/// otherwise bakes in this transaction's card digits or amount and never
/// matches again. This mirrors `ladder::compute_template_hash`, which also
/// collapses digits — so a rule keyed to a template hash stays consistent with
/// the anchor built for it.
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

/// Indian digit grouping: last three digits, then pairs. Banks print
/// "1,020.00" and "12,34,567.00", never "1,234,567.00", so an anchor built
/// against a Western-grouped needle would simply never be found.
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

/// The ways a stored value might literally appear in the source, best first.
///
/// The stored form and the printed form differ for exactly two field kinds:
/// money (minor units in the DB, grouped decimals on the page) and dates (ISO
/// in the DB, one of a dozen local formats on the page). Everything else is
/// stored as it was read, so its only candidate is itself.
///
/// A field whose value is genuinely not present in any of these forms produces
/// no candidate that [`find_ignore_case`] can locate, synthesis returns `None`,
/// and the LLM fallback gets its turn. That is the intended division of labour,
/// not a gap.
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
            // Grouped-decimal first: it is the most specific, so it anchors
            // most tightly when the bank prints it that way.
            let mut out = vec![grouped_decimal, decimal, grouped_int, int_part];
            out.dedup();
            out
        }
        "event_time" => {
            // The stored value is "YYYY-MM-DD HH:MM:SS" or "YYYY-MM-DD"; only
            // the date part is ever printed in a bank alert's transaction line.
            let date_part = v.split_whitespace().next().unwrap_or(v);
            let Ok(d) = chrono::NaiveDate::parse_from_str(date_part, "%Y-%m-%d") else {
                return vec![v.to_string()];
            };
            // Mirrors the format list `statements::row_extractor::parse_date`
            // already accepts, so anything the parser can read back is a
            // candidate here.
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

/// Builds a regex anchored on the literal text surrounding `needle` inside
/// `source`, with capture group 1 on the value itself.
///
/// Returns `None` unless the finished pattern compiles *and* re-extracts the
/// expected value from the very source it was built from. That self-check is
/// what makes an immediately-active rule safe: a pattern that cannot reproduce
/// its own training example is never stored.
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

    // No anchor on either side would make the pattern match anything at all.
    if prefix.is_empty() && suffix.is_empty() {
        return None;
    }

    let pattern = format!("(?is){prefix}(.{{1,{MAX_CAPTURE}}}?){suffix}");

    // Guards against an anchor whose own relaxation (a `\d+` that now also
    // matches part of the value) shifts the capture.
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

/// The whole deterministic pass for one corrected field.
///
/// Returns a `rule_payload_json` value — `{"regex", "capture_group"}` for a span
/// field, `{"override_value"}` for direction/currency — or `None` when no
/// self-consistent candidate exists, which is the signal to try the LLM.
pub fn synthesize(field_name: &str, source: &str, new_value: &str) -> Option<serde_json::Value> {
    if OVERRIDE_FIELDS.contains(&field_name) {
        let v = new_value.trim();
        // Closed vocabularies. An override is applied unconditionally to every
        // email matching the template, so a typo here would silently relabel a
        // whole template's worth of transactions — this is the one place a
        // free-text value must not become a rule.
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

/// Runs a stored payload against a source, returning the raw captured text.
///
/// One function for both payload shapes so extraction, the self-check and the
/// regression check can never disagree about what a rule means.
/// Compiled learned-rule patterns, keyed by the pattern string itself.
///
/// audit_02 #2: `apply_payload` compiled a fresh `Regex`
/// on every call. It is called once per live rule per message from Layer 1's
/// `apply_learned_fields`, so a 10k-message scan against 30 active rules paid
/// 300,000 compilations — and the corpus-replay validation gate below calls it
/// again for every historical sample × every candidate rule. `Regex::new` is
/// NFA construction, not a lookup.
///
/// Keyed on the pattern rather than a rule id because the same pattern reaches
/// here from three places (Layer 1, `self_check`, the replay gate) that do not
/// all have a rule id, and because two rules with identical patterns should
/// share one program.
///
/// ponytail: unbounded map, but the keys are `field_rules` rows — bounded by
/// how many rules a user has taught (tens), not by message volume. Add an LRU
/// only if a real deployment ever shows it growing.
static COMPILED_RULE_REGEXES: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<String, regex::Regex>>,
> = std::sync::OnceLock::new();

/// Compiles `pattern` once and reuses it thereafter. `Regex` clones share the
/// compiled program internally, so the clone is a refcount bump, not a rebuild.
///
/// An invalid pattern is not cached — it stays `None` and is retried. That is
/// deliberate: caching failure would need a second map for a case the
/// synthesis gate already prevents from ever being stored.
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

/// Mandatory gate step 1: the payload compiles and recovers the corrected
/// value from the exact source it was built from.
///
/// `expected_needles` is [`needle_candidates`]'s output rather than one string,
/// because the pattern was anchored on whichever printed form was actually
/// found — checking against only the stored form would reject every correct
/// amount and date rule.
pub fn self_check(payload: &serde_json::Value, source: &str, expected_needles: &[String]) -> bool {
    let Some(recovered) = apply_payload(payload, source) else {
        return false;
    };
    let recovered = recovered.trim();
    expected_needles
        .iter()
        .any(|n| recovered.eq_ignore_ascii_case(n.trim()))
}

/// Mandatory gate step 2: the candidate must not change any answer this bank's
/// history has already settled on.
///
/// Three outcomes per sample, and only one of them is a failure:
/// * the rule does not fire → fine. Rules for different template shapes coexist
///   by not matching each other's sources; that is the whole reason
///   `select_live_by_bank` is bank-wide rather than hash-scoped.
/// * the rule fires and agrees → fine, and good evidence.
/// * the rule fires and disagrees → reject the entire candidate. Old behaviour
///   for that bank is left exactly as it was.
///
/// An empty corpus (new bank, or retention swept the bodies) returns `Ok`
/// deliberately: the self-check has already proved the rule reproduces a real
/// user correction, and refusing to learn from a bank with no history would
/// mean never learning from a new bank at all.
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

/// Whether a freshly captured span means the same thing as a stored value.
///
/// A plain string compare is wrong for exactly the two fields whose stored and
/// printed forms differ — money and dates — and comparing those literally would
/// reject every correct rule for them. Reuses [`needle_candidates`], so the
/// comparison can never drift from the formats synthesis anchors on.
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

    /// audit_02 #2: `apply_payload` now serves its regexes from a process-wide
    /// cache instead of recompiling per call. A cache that returned the wrong
    /// program would silently mis-extract every learned field, so pin that
    /// repeated calls agree, that two distinct patterns don't collide on one
    /// entry, and that a pattern that fails to compile stays `None` rather
    /// than poisoning the map.
    #[test]
    fn cached_rule_regexes_stay_distinct_and_repeatable() {
        let merchant = serde_json::json!({ "regex": r"at\s+(\S+)", "capture_group": 1 });
        let last4 = serde_json::json!({ "regex": r"ending\s+(\d+)", "capture_group": 1 });

        let first = apply_payload(&merchant, SBI_BODY);
        assert_eq!(first.as_deref(), Some("RAZ*SWIGGY"));
        // Second call takes the cached path -- must produce the identical answer.
        assert_eq!(apply_payload(&merchant, SBI_BODY), first);

        // A different pattern must get its own program, not the cached one.
        assert_eq!(apply_payload(&last4, SBI_BODY).as_deref(), Some("7603"));
        assert_eq!(apply_payload(&merchant, SBI_BODY), first);

        // Uncompilable pattern: `None`, and the cache still works afterwards.
        let broken = serde_json::json!({ "regex": r"(unclosed", "capture_group": 1 });
        assert_eq!(apply_payload(&broken, SBI_BODY), None);
        assert_eq!(apply_payload(&merchant, SBI_BODY), first);

        // `override_value` short-circuits before any regex is involved.
        let override_rule = serde_json::json!({ "override_value": "Swiggy" });
        assert_eq!(
            apply_payload(&override_rule, SBI_BODY).as_deref(),
            Some("Swiggy")
        );
    }

    // ── The relaxation that makes a learned rule survive the next email ──────
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

    // ── The guard that makes skipping human approval safe ────────────────────
    #[test]
    fn synthesis_refuses_a_value_absent_from_the_source() {
        assert!(synthesize_span_regex(SBI_BODY, "ZOMATO").is_none());
        assert!(
            synthesize("merchant", SBI_BODY, "ZOMATO").is_none(),
            "a corrected value the source never contains has no anchorable span"
        );
    }

    // ── Generalisation past merchant: the whole point of this module ─────────
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

    // ── Indian digit grouping is how banks actually print amounts ────────────
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

    // ── direction/currency have no span, so they get an override instead ─────
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

    // ── The gate itself ──────────────────────────────────────────────────────
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

    // ── Gate step 2: a new rule must not change any settled answer ───────────
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
        // Captures the amount where history says the merchant was "Amazon".
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
        // A rule for one template shape simply does not fire on another; that
        // is coexistence, not regression.
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
        // History stores 24543 minor units; the rule captures the printed
        // "245.43". These agree, and a naive string compare would say otherwise.
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
