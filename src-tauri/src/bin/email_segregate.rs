use dinero_app_lib::extraction::ladder::ExtractionResult;
use dinero_app_lib::ingestion::mime_sanitization::{sanitize_html, sanitize_plain_text};

fn main() {}

/// Mirrors the private `MessageProcessor::parse_from_header` in
/// `src-tauri/src/ingestion/message_processor.rs` — that fn is not `pub`,
/// so it is unreachable from this bin crate; reimplemented verbatim.
fn parse_from_header(from: &str) -> (String, Option<String>) {
    if let (Some(start), Some(end)) = (from.find('<'), from.rfind('>')) {
        if start < end {
            let email = from[start + 1..end].trim().to_string();
            let name_part = from[..start].trim();
            let display_name = if name_part.is_empty() {
                None
            } else {
                Some(
                    name_part
                        .trim_matches(|c| c == '"' || c == ' ' || c == '\'')
                        .to_string(),
                )
            };
            return (email, display_name);
        }
    }
    (from.trim().to_string(), None)
}

/// Mirrors `MessageProcessor::evaluate_mandatory_field_gate`, which is
/// `pub(crate)` and therefore invisible across the bin/lib crate boundary.
fn evaluate_mandatory_field_gate(obs: &ExtractionResult) -> bool {
    let has_amount = obs.amount_minor.is_some();
    let has_entity = obs.merchant_raw.is_some();
    let has_balance = obs.balance_after.is_some();
    (has_amount && has_entity) || has_balance
}

/// Mirrors `MessageProcessor::gate3_failure_reason` (also `pub(crate)`).
fn gate3_failure_reason(obs: &ExtractionResult) -> &'static str {
    let has_amount = obs.amount_minor.is_some();
    let has_entity = obs.merchant_raw.is_some();
    match (has_amount, has_entity) {
        (false, _) => "gate3_failed:missing_amount",
        (true, false) => "gate3_failed:missing_counterparty",
        (true, true) => "gate3_failed",
    }
}

/// Mirrors `MessageProcessor::internal_date_fallback`, adapted for the
/// export's `internalDate` already being a JSON integer (milliseconds)
/// rather than the Gmail API's numeric-string.
fn internal_date_fallback(internal_date_ms: Option<i64>) -> Option<i64> {
    internal_date_ms.map(|ms| ms / 1000)
}

/// Mirrors `mime_sanitization::extract_body_and_attachments`'s fallback
/// logic (use `body_text` if present, else sanitize `body_html`), needed
/// here because the export's `body_text` is empty on ~32% of records.
fn effective_body(body_text: &str, body_html: &str) -> String {
    let raw = if !body_text.trim().is_empty() {
        body_text.to_string()
    } else if !body_html.trim().is_empty() {
        sanitize_html(body_html)
    } else {
        String::new()
    };
    sanitize_plain_text(&raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_from_header_extracts_display_name_and_email() {
        assert_eq!(
            parse_from_header("YES BANK Alerts <alerts@yes.bank.in>"),
            ("alerts@yes.bank.in".to_string(), Some("YES BANK Alerts".to_string()))
        );
    }

    #[test]
    fn parse_from_header_strips_quotes_from_display_name() {
        assert_eq!(
            parse_from_header("\"Bank, Inc.\" <ops@bank.com>"),
            ("ops@bank.com".to_string(), Some("Bank, Inc.".to_string()))
        );
    }

    #[test]
    fn parse_from_header_bare_address_has_no_display_name() {
        assert_eq!(
            parse_from_header("noreply@bank.com"),
            ("noreply@bank.com".to_string(), None)
        );
    }

    #[test]
    fn mandatory_field_gate_passes_on_amount_and_entity() {
        let obs = dinero_app_lib::extraction::ladder::ExtractionResult {
            amount_minor: Some(100),
            merchant_raw: Some("Store".to_string()),
            ..Default::default()
        };
        assert!(evaluate_mandatory_field_gate(&obs));
    }

    #[test]
    fn mandatory_field_gate_passes_on_balance_alone() {
        let obs = dinero_app_lib::extraction::ladder::ExtractionResult {
            balance_after: Some(500),
            ..Default::default()
        };
        assert!(evaluate_mandatory_field_gate(&obs));
    }

    #[test]
    fn mandatory_field_gate_fails_on_amount_without_entity_or_balance() {
        let obs = dinero_app_lib::extraction::ladder::ExtractionResult {
            amount_minor: Some(100),
            ..Default::default()
        };
        assert!(!evaluate_mandatory_field_gate(&obs));
    }

    #[test]
    fn gate3_failure_reason_reports_missing_amount() {
        let obs = dinero_app_lib::extraction::ladder::ExtractionResult::default();
        assert_eq!(gate3_failure_reason(&obs), "gate3_failed:missing_amount");
    }

    #[test]
    fn gate3_failure_reason_reports_missing_counterparty() {
        let obs = dinero_app_lib::extraction::ladder::ExtractionResult {
            amount_minor: Some(100),
            ..Default::default()
        };
        assert_eq!(gate3_failure_reason(&obs), "gate3_failed:missing_counterparty");
    }

    #[test]
    fn internal_date_fallback_converts_ms_to_seconds() {
        assert_eq!(internal_date_fallback(Some(1783697136000)), Some(1783697136));
    }

    #[test]
    fn internal_date_fallback_none_stays_none() {
        assert_eq!(internal_date_fallback(None), None);
    }

    #[test]
    fn effective_body_prefers_nonempty_body_text() {
        assert_eq!(effective_body("Hello World", "<p>ignored</p>"), "Hello World");
    }

    #[test]
    fn effective_body_falls_back_to_sanitized_html_when_text_empty() {
        let body = effective_body("", "<p>Hi there</p>");
        assert!(body.contains("Hi there"));
        assert!(!body.contains('<'));
    }

    #[test]
    fn effective_body_both_empty_yields_empty_string() {
        assert_eq!(effective_body("", ""), "");
    }
}
