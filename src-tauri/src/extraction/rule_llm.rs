//! LLM fallback for rule authoring (design 2026-07-29).
//!
//! Reached only when [`crate::extraction::rule_synthesis::synthesize`] cannot
//! produce a self-consistent candidate — in practice when a bank has
//! restructured its template enough that the corrected value no longer appears
//! in any form the deterministic pass knows how to look for. The deterministic
//! pass covers the large majority of real corrections ("one field's span
//! moved") instantly and for free, so this path is the exception.
//!
//! The model's output earns no special trust: it goes through the identical
//! [`crate::extraction::rule_synthesis::self_check`] a deterministic candidate
//! does, and then the identical regression check. That gate — not the author —
//! is what makes skipping human approval safe.

use serde::Deserialize;
#[derive(Debug, Deserialize)]
struct RuleLlmOutput {
    regex: Option<String>,
    capture_group: Option<u64>,
}

/// Grammar-constrained output shape. Both fields are required, so a
/// half-answered completion is unrepresentable rather than something to repair
/// after the fact.
pub fn authoring_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "regex": {"type": "string"},
            "capture_group": {"type": "integer"}
        },
        "required": ["regex", "capture_group"]
    })
}
pub fn generate_prompt(
    field_name: &str,
    bank_name: &str,
    source: &str,
    old_value: Option<&str>,
    new_value: &str,
    existing_rules: &[String],
) -> String {
    let existing = if existing_rules.is_empty() {
        "(none yet)".to_string()
    } else {
        existing_rules
            .iter()
            .map(|r| format!("  - {r}"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        "You are writing a regular expression that extracts one field from a bank's \
         notification text.\n\n\
         An automated parser read the message below and produced the wrong value for \
         the field \"{field}\". A user corrected it. Write a regex that would have \
         produced the corrected value from this message, and that will keep working on \
         the next message of the same kind.\n\n\
         Return ONLY valid JSON with these two fields:\n\
         - regex: a Rust `regex` crate pattern. Use `(?is)` at the start if you need \
           case-insensitive or dot-matches-newline behaviour.\n\
         - capture_group: the 1-based index of the group containing the value.\n\n\
         Rules:\n\
         - The regex MUST extract exactly \"{new}\" from the message below. It is \
           checked mechanically; a pattern that does not is discarded.\n\
         - Anchor on the surrounding wording, not on this message's specific numbers. \
           Write `\\d+` where digits vary (amounts, card digits, dates, reference \
           numbers) so the pattern still matches the next message.\n\
         - Write `\\s+` between words rather than a literal space; the same message \
           renders with different whitespace in different mail clients.\n\
         - Escape regex metacharacters that appear literally in the text (`*`, `.`, \
           `(`, `+`, `?`, `$`).\n\
         - Keep the capture bounded (for example `(.{{1,80}}?)`) so a mis-anchored \
           pattern cannot swallow the rest of the message.\n\n\
         Bank: {bank}\n\
         Field to extract: {field}\n\
         Parser's wrong value: {old}\n\
         User's corrected value: {new}\n\
         Rules already learned for this bank:\n{existing}\n\
         Message:\n\
         \"\"\"\n{source}\n\"\"\"\n\
         JSON Output:",
        field = field_name,
        bank = bank_name,
        old = old_value.unwrap_or("(the parser found nothing)"),
        new = new_value,
        existing = existing,
        source = source,
    )
}

/// Validates one raw completion against the source.
///
/// Returns `None` for every failure mode — unparseable, uncompilable, a capture
/// group that does not exist, or a pattern that does not recover the corrected
/// value from the exact source it was written for. A rejected answer writes no
/// rule and leaves the user's correction untouched.
pub fn validate(
    raw_output: &str,
    field_name: &str,
    source: &str,
    new_value: &str,
) -> Option<serde_json::Value> {
    let json_text = crate::extraction::llm::LlmEngine::extract_json_block(raw_output)?;
    let parsed: RuleLlmOutput = serde_json::from_str(json_text).ok()?;
    let regex = parsed.regex?;
    let group = parsed.capture_group.unwrap_or(1);
    if regex.trim().is_empty() || group == 0 {
        return None;
    }

    let payload = serde_json::json!({ "regex": regex, "capture_group": group });

    // The identical gate a deterministic candidate faces. `needle_candidates`
    // rather than `new_value` alone, because an amount or date rule correctly
    // captures the *printed* form, not the stored one.
    let needles = crate::extraction::rule_synthesis::needle_candidates(field_name, new_value);
    if !crate::extraction::rule_synthesis::self_check(&payload, source, &needles) {
        tracing::debug!(
            field = field_name,
            "rule authoring: LLM pattern failed the self-check, discarding"
        );
        return None;
    }

    Some(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: &str = "Dear Customer, INR 1,020.00 was debited from your account \
                        ending 4412 towards MERCHANT: BLUE TOKAI COFFEE on 14/07/26.";

    #[test]
    fn accepts_a_regex_that_recovers_the_corrected_value() {
        let raw = r#"{"regex": "MERCHANT:\\s*(.{1,80}?)\\s+on", "capture_group": 1}"#;
        let payload = validate(raw, "merchant", BODY, "BLUE TOKAI COFFEE")
            .expect("a self-consistent regex must be accepted");
        assert_eq!(payload["capture_group"], 1);
        assert_eq!(
            crate::extraction::rule_synthesis::apply_payload(&payload, BODY)
                .unwrap()
                .trim(),
            "BLUE TOKAI COFFEE"
        );
    }

    /// The guard that makes "the LLM wrote it" carry no extra trust.
    #[test]
    fn rejects_a_regex_that_recovers_the_wrong_span() {
        let raw = r#"{"regex": "ending\\s+(\\d+)", "capture_group": 1}"#;
        assert!(
            validate(raw, "merchant", BODY, "BLUE TOKAI COFFEE").is_none(),
            "an LLM-authored regex must clear the same self-check as a deterministic one"
        );
    }

    #[test]
    fn rejects_an_uncompilable_regex() {
        let raw = r#"{"regex": "MERCHANT:(unclosed", "capture_group": 1}"#;
        assert!(validate(raw, "merchant", BODY, "BLUE TOKAI COFFEE").is_none());
    }

    #[test]
    fn rejects_unparseable_output() {
        assert!(validate("I could not find one, sorry.", "merchant", BODY, "X").is_none());
    }

    #[test]
    fn tolerates_output_wrapped_in_prose_or_fences() {
        let raw = "Here you go:\n```json\n{\"regex\": \"MERCHANT:\\\\s*(.{1,80}?)\\\\s+on\", \
                   \"capture_group\": 1}\n```\nHope that helps.";
        assert!(
            validate(raw, "merchant", BODY, "BLUE TOKAI COFFEE").is_some(),
            "a fenced or prose-wrapped answer must still be usable"
        );
    }

    /// An amount rule anchors on the printed form, not the stored minor units.
    #[test]
    fn accepts_an_amount_regex_matching_the_printed_form() {
        let raw = r#"{"regex": "INR\\s+([\\d,.]+)\\s+was", "capture_group": 1}"#;
        assert!(
            validate(raw, "amount", BODY, "102000").is_some(),
            "1,020.00 in the body is 102000 minor units in the DB"
        );
    }

    #[test]
    fn rejects_a_capture_group_that_does_not_exist() {
        let raw = r#"{"regex": "MERCHANT:\\s*(.{1,80}?)\\s+on", "capture_group": 7}"#;
        assert!(validate(raw, "merchant", BODY, "BLUE TOKAI COFFEE").is_none());
    }

    #[test]
    fn schema_forces_the_two_fields_we_read() {
        let schema = authoring_schema();
        assert_eq!(
            schema["required"],
            serde_json::json!(["regex", "capture_group"]),
            "the grammar must make a missing field unrepresentable"
        );
    }

    #[test]
    fn prompt_states_the_corrected_value_and_the_existing_rules() {
        let prompt = generate_prompt(
            "merchant",
            "HDFC Bank",
            BODY,
            Some("ending 4412 towards"),
            "BLUE TOKAI COFFEE",
            &[r"at (.+?) on".to_string()],
        );
        assert!(prompt.contains("BLUE TOKAI COFFEE"));
        assert!(prompt.contains("HDFC Bank"));
        assert!(prompt.contains("merchant"));
        assert!(prompt.contains(r"at (.+?) on"), "the current ruleset must be in the prompt");
    }

    #[test]
    fn long_sources_are_not_truncated() {
        let long = "x".repeat(10_000);
        let prompt = generate_prompt("merchant", "HDFC", &long, None, "X", &[]);
        assert!(prompt.contains(&long));
        assert!(!prompt.contains("[...truncated...]"));
    }
}
