//! Uses the LLM to author extraction rules, not to extract values.
//!
//! The leverage here is one-off cost: a rule synthesised once from a single
//! message then handles every future message sharing that template for free.
//! Authored rules are validated and regression-checked before being trusted.
use serde::Deserialize;
#[derive(Debug, Deserialize)]
struct RuleLlmOutput {
    regex: Option<String>,
    capture_group: Option<u64>,
}

/// JSON schema constraining the model's rule-authoring output.
///
/// Forces a parseable structure, so the response can be validated as a rule
/// rather than interpreted as prose.
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
/// Builds the rule-authoring prompt from a message and its known values.
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

/// Validates an authored rule before it is trusted.
///
/// A schema guarantees shape, not correctness. The rule is compiled and executed
/// here, because a syntactically valid pattern can still fail to match or capture
/// the wrong span.
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
        assert!(
            prompt.contains(r"at (.+?) on"),
            "the current ruleset must be in the prompt"
        );
    }

    #[test]
    fn long_sources_are_not_truncated() {
        let long = "x".repeat(10_000);
        let prompt = generate_prompt("merchant", "HDFC", &long, None, "X", &[]);
        assert!(prompt.contains(&long));
        assert!(!prompt.contains("[...truncated...]"));
    }
}
