#[cfg(test)]
mod tests {
    use crate::extraction::llm::LlmEngine;
    use crate::llm_manager::{download_model, LlmModelInfo};
    use mockito::Server;
    use std::env::temp_dir;
    use std::path::PathBuf;
    use tokio::fs;

    // --------------------------------------------------------------------------------
    // 11.1 Candle LLM Runtime Integration & Manager Tests
    // --------------------------------------------------------------------------------

    #[tokio::test]
    async fn test_11_1_download_model_happy_path() {
        let mut server = Server::new_async().await;
        let url = server.url();

        let model_content = b"valid gguf model content";
        let mock_gguf = server
            .mock("GET", "/model.gguf")
            .with_status(200)
            .with_body(model_content)
            .create_async()
            .await;

        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(model_content);
        let expected_hash = format!("{:x}", hasher.finalize());

        let app_dir = temp_dir().join(uuid::Uuid::new_v4().to_string());

        let info = LlmModelInfo {
            id: "test-model-valid".to_string(),
            name: "Test Model".to_string(),
            tag: "test:valid".to_string(),
            tier: 1,
            min_ram_gb: 8.0,
            approx_size_gb: 1.0,
            rationale: "test".to_string(),
            gguf_url: format!("{}/model.gguf", url),
            expected_sha256: expected_hash,
            tokenizer_url: None,
        };

        let result = download_model(&app_dir, &info).await;
        mock_gguf.assert_async().await;
        assert!(result.is_ok(), "Download should succeed for valid hash");

        let path = crate::llm_manager::get_model_path(&app_dir, "test-model-valid");
        assert!(path.is_some());
        assert!(path.unwrap().exists());

        fs::remove_dir_all(&app_dir).await.ok();
    }

    #[tokio::test]
    async fn test_11_1_download_model_corrupted_hash_cleanup() {
        let mut server = Server::new_async().await;
        let url = server.url();

        let model_content = b"corrupted gguf content";
        let mock_gguf = server
            .mock("GET", "/model.gguf")
            .with_status(200)
            .with_body(model_content)
            .create_async()
            .await;

        let app_dir = temp_dir().join(uuid::Uuid::new_v4().to_string());

        let info = LlmModelInfo {
            id: "test-model-invalid".to_string(),
            name: "Test Invalid Model".to_string(),
            tag: "test:invalid".to_string(),
            tier: 1,
            min_ram_gb: 8.0,
            approx_size_gb: 1.0,
            rationale: "test".to_string(),
            gguf_url: format!("{}/model.gguf", url),
            expected_sha256: "badhash1234567890".to_string(),
            tokenizer_url: None,
        };

        let result = download_model(&app_dir, &info).await;
        mock_gguf.assert_async().await;
        assert!(result.is_err(), "Download must fail when hash doesn't match");

        // The corrupted file MUST be deleted.
        let path = app_dir.join("models").join("test-model-invalid.gguf");
        assert!(!path.exists(), "Corrupted download file must be deleted");

        fs::remove_dir_all(&app_dir).await.ok();
    }

    #[tokio::test]
    async fn test_11_1_download_model_server_error() {
        let mut server = Server::new_async().await;
        let url = server.url();

        let mock_gguf = server
            .mock("GET", "/model.gguf")
            .with_status(500)
            .create_async()
            .await;

        let app_dir = temp_dir().join(uuid::Uuid::new_v4().to_string());

        let info = LlmModelInfo {
            id: "test-model-500".to_string(),
            name: "Test 500".to_string(),
            tag: "test:500".to_string(),
            tier: 1,
            min_ram_gb: 8.0,
            approx_size_gb: 1.0,
            rationale: "test".to_string(),
            gguf_url: format!("{}/model.gguf", url),
            expected_sha256: "irrelevant-never-reached".to_string(),
            tokenizer_url: None,
        };

        let result = download_model(&app_dir, &info).await;
        mock_gguf.assert_async().await;
        assert!(result.is_err(), "Download must fail on 500 server error");
        
        fs::remove_dir_all(&app_dir).await.ok();
    }

    #[tokio::test]
    async fn test_11_1_download_model_with_tokenizer() {
        let mut server = Server::new_async().await;
        let url = server.url();

        let model_content = b"gguf content";
        let tokenizer_content = b"tokenizer content";
        let mock_gguf = server
            .mock("GET", "/model.gguf")
            .with_status(200)
            .with_body(model_content)
            .create_async()
            .await;
        let mock_tok = server
            .mock("GET", "/tok.json")
            .with_status(200)
            .with_body(tokenizer_content)
            .create_async()
            .await;

        let app_dir = temp_dir().join(uuid::Uuid::new_v4().to_string());

        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(model_content);
        let expected_hash = format!("{:x}", hasher.finalize());

        let info = LlmModelInfo {
            id: "test-model-tok".to_string(),
            name: "Test Tokenizer".to_string(),
            tag: "test:tok".to_string(),
            tier: 1,
            min_ram_gb: 8.0,
            approx_size_gb: 1.0,
            rationale: "test".to_string(),
            gguf_url: format!("{}/model.gguf", url),
            expected_sha256: expected_hash,
            tokenizer_url: Some(format!("{}/tok.json", url)),
        };

        let result = download_model(&app_dir, &info).await;
        mock_gguf.assert_async().await;
        mock_tok.assert_async().await;
        
        assert!(result.is_ok(), "Download should succeed");

        let path = crate::llm_manager::get_tokenizer_path(&app_dir, "test-model-tok");
        assert!(path.is_some());
        assert!(path.unwrap().exists());

        fs::remove_dir_all(&app_dir).await.ok();
    }

    #[test]
    fn test_11_1_graceful_degradation_on_oom_or_missing_file() {
        // Simulating an OOM or missing file by providing an invalid path.
        // LlmEngine::extract MUST not crash the app, but return None.
        let engine = LlmEngine::new(&PathBuf::from("/invalid/path.gguf"), &PathBuf::from("/invalid/tok.json"));
        
        let result = engine.extract("You spent Rs 500 at Amazon.");
        assert!(result.is_none(), "Engine must return None and degrade gracefully when model fails to load");
    }

    // --------------------------------------------------------------------------------
    // 11.2 LLM Extraction Prompt Design & Parsers Tests
    // --------------------------------------------------------------------------------

    #[test]
    fn test_11_2_prompt_generation_constrained() {
        let body = "You spent Rs 1500 at Starbucks on 12-May-2023.";
        let prompt = LlmEngine::generate_prompt(body);

        assert!(prompt.contains(body), "Prompt must contain the sanitized email body");
        assert!(prompt.contains("amount: number"), "Prompt must request amount");
        assert!(prompt.contains("currency: string"), "Prompt must request currency");
        assert!(prompt.contains("direction: string"), "Prompt must request direction");
        assert!(prompt.contains("merchant: string"), "Prompt must request merchant");
        assert!(prompt.contains("event_time: integer"), "Prompt must request event_time");
        assert!(prompt.contains("reference_id: string"), "Prompt must request reference_id");
        
        // Security constraint check
        assert!(!prompt.to_lowercase().contains("profile"), "Prompt must not leak user profile data");
        assert!(!prompt.to_lowercase().contains("metadata"), "Prompt must not contain system metadata instructions");
    }

    #[test]
    fn test_11_2_parse_valid_json() {
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
    fn test_11_2_parse_json_with_markdown_chatter() {
        let engine = LlmEngine::new(&PathBuf::from("dummy"), &PathBuf::from("dummy"));
        
        let chatty_json = r#"
        Certainly! Here is the JSON you requested:
        ```json
        {
            "amount": 50.0,
            "currency": "USD",
            "direction": "debit",
            "merchant": "Netflix",
            "event_time": 1704067200,
            "reference_id": "NET123"
        }
        ```
        Let me know if you need anything else!
        "#;
        
        let result = engine.parse_json_to_result(chatty_json).expect("Should parse chatty json successfully");
        
        assert_eq!(result.amount_minor, Some(5000));
        assert_eq!(result.merchant_raw, Some("Netflix".to_string()));
    }

    #[test]
    fn test_11_2_parse_direction_normalization() {
        let engine = LlmEngine::new(&PathBuf::from("dummy"), &PathBuf::from("dummy"));
        
        let json_credit = r#"{
            "amount": 100.0,
            "currency": "USD",
            "direction": "CREDIT",
            "merchant": "Refund",
            "event_time": 1704067200
        }"#;
        
        let result_credit = engine.parse_json_to_result(json_credit).expect("Should parse");
        assert_eq!(result_credit.direction, Some("credit".to_string()), "CREDIT should normalize to lowercase credit");

        let json_debit = r#"{
            "amount": 100.0,
            "currency": "USD",
            "direction": "DEBIT",
            "merchant": "Purchase",
            "event_time": 1704067200
        }"#;
        
        let result_debit = engine.parse_json_to_result(json_debit).expect("Should parse");
        assert_eq!(result_debit.direction, Some("debit".to_string()), "DEBIT should normalize to lowercase debit");

        let json_unknown = r#"{
            "amount": 100.0,
            "currency": "USD",
            "direction": "UNKNOWN",
            "merchant": "Purchase",
            "event_time": 1704067200
        }"#;
        
        let result_unknown = engine.parse_json_to_result(json_unknown).expect("Should parse");
        assert_eq!(result_unknown.direction, Some("debit".to_string()), "Unknown direction should default to debit");
    }

    #[test]
    fn test_11_2_parse_missing_mandatory_fields_rejected() {
        let engine = LlmEngine::new(&PathBuf::from("dummy"), &PathBuf::from("dummy"));
        
        let json_missing_currency = r#"{
            "amount": 50.0,
            "direction": "debit",
            "merchant": "Netflix",
            "event_time": 1704067200
        }"#; 
        
        assert!(engine.parse_json_to_result(json_missing_currency).is_none(), "Output missing currency must be rejected");

        let json_missing_amount = r#"{
            "currency": "USD",
            "direction": "debit",
            "merchant": "Netflix",
            "event_time": 1704067200
        }"#; 
        
        assert!(engine.parse_json_to_result(json_missing_amount).is_none(), "Output missing amount must be rejected");

        let json_missing_direction = r#"{
            "amount": 50.0,
            "currency": "USD",
            "merchant": "Netflix",
            "event_time": 1704067200
        }"#; 
        
        assert!(engine.parse_json_to_result(json_missing_direction).is_none(), "Output missing direction must be rejected");
    }

    #[test]
    fn test_11_2_parse_hallucinated_invalid_json_rejected() {
        let engine = LlmEngine::new(&PathBuf::from("dummy"), &PathBuf::from("dummy"));
        
        let hallucinated_text = "I'm sorry, I cannot extract the information you requested.";
        let result = engine.parse_json_to_result(hallucinated_text);
        assert!(result.is_none(), "Completely hallucinated non-JSON output must return None");

        let malformed_json = r#"{
            "amount": 50.0,
            "currency": "USD"
            "merchant": "Netflix
        "#;
        let result2 = engine.parse_json_to_result(malformed_json);
        assert!(result2.is_none(), "Malformed JSON output must return None");
    }
}
