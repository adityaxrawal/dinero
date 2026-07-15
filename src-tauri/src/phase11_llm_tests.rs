use crate::extraction::llm::LlmEngine;
use std::path::PathBuf;

#[test]
fn test_llm_prompt_generation() {
    let body = "You spent Rs 500 at Starbucks on 12-May-2023.";
    let prompt = LlmEngine::generate_prompt(body);

    assert!(prompt.contains(body), "Prompt must contain the sanitized email body");
    assert!(prompt.contains("amount: number"), "Prompt must request amount");
    assert!(prompt.contains("currency: string"), "Prompt must request currency");
    assert!(prompt.contains("direction: string"), "Prompt must request direction");
    assert!(prompt.contains("merchant: string"), "Prompt must request merchant");
    assert!(prompt.contains("event_time: integer"), "Prompt must request event_time");
    
    // Ensure no metadata instructions like user profile are present
    assert!(!prompt.to_lowercase().contains("profile"), "Prompt must not contain user profile data");
}

#[test]
fn test_extract_json_block_clean() {
    let output = r#"{"amount": 500.0, "currency": "INR", "direction": "debit"}"#;
    let block = LlmEngine::extract_json_block(output).unwrap();
    assert_eq!(block, output);
}

#[test]
fn test_extract_json_block_with_markdown_and_chatter() {
    let output = r#"
Sure! Here is the extracted JSON:
```json
{
  "amount": 1500.5,
  "currency": "USD",
  "direction": "debit",
  "merchant": "Amazon",
  "event_time": 1704067200,
  "reference_id": "9999"
}
```
Have a nice day!
    "#;
    
    let block = LlmEngine::extract_json_block(output).unwrap();
    assert!(block.starts_with('{'));
    assert!(block.ends_with('}'));
    assert!(block.contains("\"amount\": 1500.5"));
}

#[test]
fn test_llm_output_parsed_to_extraction_result() {
    let engine = LlmEngine::new(&PathBuf::from("dummy"), &PathBuf::from("dummy"));
    
    let valid_json = r#"{
        "amount": 1500.50,
        "currency": "INR",
        "direction": "debit",
        "merchant": "Amazon",
        "event_time": 1704067200,
        "reference_id": "ABC123XYZ"
    }"#;
    
    let result = engine.parse_json_to_result(valid_json).expect("Should parse successfully");
    
    assert_eq!(result.amount_minor, Some(150050));
    assert_eq!(result.currency, Some("INR".to_string()));
    assert_eq!(result.direction, Some("debit".to_string()));
    assert_eq!(result.merchant_raw, Some("Amazon".to_string()));
    assert_eq!(result.event_time, Some(1704067200));
    assert_eq!(result.reference_id, Some("ABC123XYZ".to_string()));
    assert_eq!(result.extraction_method, "llm_layer6");
}

#[test]
fn test_llm_output_parsed_invalid_direction_normalized() {
    let engine = LlmEngine::new(&PathBuf::from("dummy"), &PathBuf::from("dummy"));
    
    let json = r#"{
        "amount": 50.0,
        "currency": "USD",
        "direction": "DEBIT",
        "merchant": "Netflix",
        "event_time": 1704067200
    }"#;
    
    let result = engine.parse_json_to_result(json).expect("Should parse");
    assert_eq!(result.direction, Some("debit".to_string()), "Direction should be normalized to lowercase");
}

#[test]
fn test_llm_output_parsed_missing_mandatory_fields_rejected() {
    let engine = LlmEngine::new(&PathBuf::from("dummy"), &PathBuf::from("dummy"));
    
    let json = r#"{
        "amount": 50.0,
        "merchant": "Netflix"
    }"#; // Missing currency, direction, event_time
    
    let result = engine.parse_json_to_result(json);
    assert!(result.is_none(), "Output missing mandatory fields must be rejected");
}

#[test]
fn test_llm_extraction_falls_back_gracefully_on_oom() {
    // We simulate an OOM or missing file by passing a non-existent path.
    // The engine's thread should panic or return Err, which is caught by catch_unwind
    // and mapped to None, gracefully degrading rather than crashing the app.
    
    let engine = LlmEngine::new(&PathBuf::from("/does/not/exist.gguf"), &PathBuf::from("/does/not/exist.json"));
    
    let result = engine.extract("You spent Rs 500 at Amazon.");
    assert!(result.is_none(), "Engine must return None and degrade gracefully when model fails to load");
}

#[test]
fn test_llm_output_parsed_with_hallucinated_json_rejected() {
    let engine = LlmEngine::new(&PathBuf::from("dummy"), &PathBuf::from("dummy"));
    
    let json = r#"This is not JSON at all."#;
    
    let result = engine.parse_json_to_result(json);
    assert!(result.is_none(), "Invalid JSON output must be handled and rejected");
}
