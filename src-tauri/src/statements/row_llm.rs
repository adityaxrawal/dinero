//! LLM fallback for statement pages the deterministic parser could not read.
//!
//! Gated behind a cheap heuristic that first asks whether a page even looks like
//! a transaction table, so cover pages, terms and marketing inserts never reach
//! the model. Output is schema-constrained and validated afterwards.
use crate::statements::parser::ParsedPage;
use crate::statements::row_extractor::{
    extract_reference_id, is_excluded_row, parse_amount_minor, parse_date, StatementRow,
};
const MAX_LLM_PAGES: usize = 12;

const MIN_TABLE_SIGNALS: usize = 2;

/// Schema constraining the model's row-extraction output.
pub fn rows_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "rows": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "date": { "type": "string" },
                        "description": { "type": "string" },
                        "amount": { "type": "string" },
                        "direction": { "enum": ["debit", "credit"] }
                    },
                    "required": ["date", "description", "amount", "direction"]
                }
            }
        },
        "required": ["rows"]
    })
}

/// Builds the row-extraction prompt for one page.
pub fn generate_prompt(issuer: &str, page_text: &str) -> String {
    format!(
        "You are reading one page of a {issuer} bank statement. Extract every \
transaction row from the table below.\n\n\
Rules:\n\
- Copy the amount exactly as printed, including commas and decimals. Do not \
reformat it.\n\
- Copy the description exactly as printed. Do not expand abbreviations, \
correct spelling, or add a merchant name that is not written there.\n\
- Use \"debit\" for money leaving the account and \"credit\" for money \
arriving. A DR marker, or a purchase, is a debit; a CR marker, a refund, or a \
payment received is a credit.\n\
- Skip opening and closing balances, totals, subtotals, and reward-point \
lines. They are not transactions.\n\
- If the page contains no transaction table, return an empty list.\n\
- Never invent a row. Every row you return must be visible in the text below.\n\n\
STATEMENT PAGE:\n{page_text}"
    )
}

/// Cheap heuristic for whether a page could hold a transaction table.
///
/// Gates the expensive model call, so cover pages, terms and marketing inserts
/// never reach it.
pub fn looks_like_a_transaction_table(text: &str) -> bool {
    let dates = regex::Regex::new(r"\d{1,2}[/\-][A-Za-z0-9]{2,3}[/\-]\d{2,4}")
        .map(|re| re.find_iter(text).count())
        .unwrap_or(0);
    let amounts = regex::Regex::new(r"\d[\d,]*\.\d{2}")
        .map(|re| re.find_iter(text).count())
        .unwrap_or(0);
    dates >= MIN_TABLE_SIGNALS && amounts >= MIN_TABLE_SIGNALS
}

/// Validates model-extracted rows against the page they came from.
///
/// Each row is checked for grounding in the actual page text, because a schema
/// guarantees shape rather than truth -- and a fabricated transaction reaching
/// the ledger is the failure that matters most here.
pub fn validate(raw_output: &str, page_text: &str, start_index: usize) -> Vec<StatementRow> {
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(raw_output) else {
        tracing::debug!("row_llm: output was not valid JSON");
        return Vec::new();
    };
    let Some(rows) = parsed.get("rows").and_then(|r| r.as_array()) else {
        return Vec::new();
    };

    let haystack = collapse(page_text);
    let mut out = Vec::new();

    for row in rows {
        let (Some(date_raw), Some(description), Some(amount_raw), Some(direction)) = (
            row.get("date").and_then(|v| v.as_str()),
            row.get("description").and_then(|v| v.as_str()),
            row.get("amount").and_then(|v| v.as_str()),
            row.get("direction").and_then(|v| v.as_str()),
        ) else {
            continue;
        };

        let description = description.trim();
        if description.is_empty() || is_excluded_row(description) {
            continue;
        }
        if !haystack.contains(&collapse(amount_raw)) || !haystack.contains(&collapse(description)) {
            tracing::debug!("row_llm: dropped a row not present in the page text");
            continue;
        }
        let (Some(transaction_date), Some(amount_minor)) =
            (parse_date(date_raw), parse_amount_minor(amount_raw))
        else {
            continue;
        };
        if amount_minor <= 0 || !matches!(direction, "debit" | "credit") {
            continue;
        }

        out.push(StatementRow {
            transaction_date,
            merchant_raw: description.to_string(),
            amount_minor,
            currency: "INR".to_string(),
            direction: direction.to_string(),
            reference_id: extract_reference_id(description),
            row_index: start_index + out.len(),
            llm_extracted: true,
        });
    }
    out
}

/// Collapses whitespace for comparison against source text.
fn collapse(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_uppercase()
}

/// Runs the model over pages the deterministic parser could not read.
pub async fn extract_unparsed_pages(
    pages: &[ParsedPage],
    parser: crate::statements::row_extractor::BankParser,
    issuer: &str,
    app_dir: &std::path::Path,
    model_id: &str,
    start_index: usize,
) -> Vec<StatementRow> {
    let mut out = Vec::new();
    let mut sent = 0usize;

    for page in pages {
        if sent >= MAX_LLM_PAGES {
            tracing::warn!(
                "row_llm: reached the {}-page inference ceiling; later pages were not sent",
                MAX_LLM_PAGES
            );
            break;
        }
        let already =
            crate::statements::row_extractor::extract_rows(std::slice::from_ref(page), parser)
                .unwrap_or_default();
        if !already.is_empty() || !looks_like_a_transaction_table(&page.text) {
            continue;
        }

        sent += 1;
        let prompt = generate_prompt(issuer, &page.text);
        let raw = match crate::llama_sidecar::complete_with_schema_and_context(
            app_dir,
            model_id,
            &prompt,
            rows_schema(),
            crate::logging::llm_logger::LlmCallContext::new(
                crate::logging::llm_logger::LlmCallType::StatementRowExtraction,
                1,
            ),
        )
        .await
        {
            Ok(raw) => raw,
            Err(e) => {
                tracing::warn!(
                    "row_llm: inference failed on page {}: {e}",
                    page.page_number
                );
                continue;
            }
        };

        let rows = validate(&raw, &page.text, start_index + out.len());
        tracing::info!(
            "row_llm: page {} yielded {} verified rows",
            page.page_number,
            rows.len()
        );
        out.extend(rows);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = "\
01/12/2025  SWIGGY ORDER BANGALORE            1,250.00  DR
15/12/2025  PAYMENT - THANK YOU              20,000.00  CR
Closing Balance                              18,750.00";

    fn output(rows: &str) -> String {
        format!(r#"{{"rows":[{rows}]}}"#)
    }

    #[test]
    fn a_faithful_transcription_is_accepted() {
        let raw = output(
            r#"{"date":"01/12/2025","description":"SWIGGY ORDER BANGALORE","amount":"1,250.00","direction":"debit"}"#,
        );
        let rows = validate(&raw, PAGE, 0);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].transaction_date, "2025-12-01");
        assert_eq!(rows[0].amount_minor, 125_000);
        assert_eq!(rows[0].direction, "debit");
        assert!(rows[0].llm_extracted, "must be tagged as LLM-extracted");
    }

    #[test]
    fn an_invented_transaction_is_rejected() {
        let raw = output(
            r#"{"date":"03/12/2025","description":"AMAZON INDIA","amount":"999.00","direction":"debit"}"#,
        );
        assert!(validate(&raw, PAGE, 0).is_empty());
    }

    #[test]
    fn a_real_merchant_with_an_invented_amount_is_rejected() {
        let raw = output(
            r#"{"date":"01/12/2025","description":"SWIGGY ORDER BANGALORE","amount":"9,999.00","direction":"debit"}"#,
        );
        assert!(validate(&raw, PAGE, 0).is_empty());
    }

    #[test]
    fn balance_rows_are_excluded() {
        let raw = output(
            r#"{"date":"15/12/2025","description":"Closing Balance","amount":"18,750.00","direction":"debit"}"#,
        );
        assert!(validate(&raw, PAGE, 0).is_empty());
    }

    #[test]
    fn column_padding_does_not_defeat_the_presence_check() {
        let padded = "05/12/2025      NETFLIX      INDIA          649.00  DR";
        let raw = output(
            r#"{"date":"05/12/2025","description":"NETFLIX INDIA","amount":"649.00","direction":"debit"}"#,
        );
        assert_eq!(validate(&raw, padded, 0).len(), 1);
    }

    #[test]
    fn row_indices_continue_from_the_deterministic_parser() {
        let raw = output(
            r#"{"date":"01/12/2025","description":"SWIGGY ORDER BANGALORE","amount":"1,250.00","direction":"debit"},
               {"date":"15/12/2025","description":"PAYMENT - THANK YOU","amount":"20,000.00","direction":"credit"}"#,
        );
        let rows = validate(&raw, PAGE, 7);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].row_index, 7);
    }

    #[test]
    fn malformed_output_yields_nothing_rather_than_panicking() {
        assert!(validate("not json at all", PAGE, 0).is_empty());
        assert!(validate(r#"{"rows":"not an array"}"#, PAGE, 0).is_empty());
        assert!(validate(r#"{"rows":[{"date":"bad"}]}"#, PAGE, 0).is_empty());
    }

    #[test]
    fn only_pages_that_look_like_tables_are_sent() {
        assert!(looks_like_a_transaction_table(PAGE));
        assert!(!looks_like_a_transaction_table(
            "Thank you for banking with us. Terms and conditions apply."
        ));
        assert!(!looks_like_a_transaction_table(
            "Statement date 01/12/2025 Total amount due 3,903.00"
        ));
    }

    #[test]
    fn a_zero_or_negative_amount_is_rejected() {
        let page = "01/12/2025  SOME MERCHANT  0.00  DR";
        let raw = output(
            r#"{"date":"01/12/2025","description":"SOME MERCHANT","amount":"0.00","direction":"debit"}"#,
        );
        assert!(validate(&raw, page, 0).is_empty());
    }
}
