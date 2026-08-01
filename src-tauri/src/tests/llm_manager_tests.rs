#[cfg(test)]
mod tests {
    use crate::llm_manager::{download_model, LlmModelInfo};
    use mockito::Server;
    use std::env::temp_dir;
    use tokio::fs;

    #[tokio::test]
    async fn test_download_model_with_valid_hash() {
        let mut server = Server::new_async().await;
        let url = server.url();

        let model_content = b"dummy gguf content";
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
            name: "Test".to_string(),
            tag: "test:valid".to_string(),
            tier: 1,
            min_ram_gb: 8.0,
            approx_size_gb: 1.0,
            rationale: "test".to_string(),
            gguf_url: format!("{}/model.gguf", url),
            expected_sha256: expected_hash,
        };

        let result = download_model(&app_dir, &info, None, None).await;
        mock_gguf.assert_async().await;

        assert!(result.is_ok(), "Download should succeed for valid hash");

        let path = crate::llm_manager::get_model_path(&app_dir, "test-model-valid");
        assert!(path.is_some());
        assert!(path.unwrap().exists());

        fs::remove_dir_all(&app_dir).await.ok();
    }

    #[tokio::test]
    async fn test_download_model_with_invalid_hash_fails_and_cleans_up() {
        let mut server = Server::new_async().await;
        let url = server.url();

        let model_content = b"dummy gguf content";
        let mock_gguf = server
            .mock("GET", "/model.gguf")
            .with_status(200)
            .with_body(model_content)
            .create_async()
            .await;

        let app_dir = temp_dir().join(uuid::Uuid::new_v4().to_string());

        let info = LlmModelInfo {
            id: "test-model-invalid".to_string(),
            name: "Test".to_string(),
            tag: "test:invalid".to_string(),
            tier: 1,
            min_ram_gb: 8.0,
            approx_size_gb: 1.0,
            rationale: "test".to_string(),
            gguf_url: format!("{}/model.gguf", url),
            expected_sha256: "badhash123".to_string(), // purposely invalid hash
        };

        let result = download_model(&app_dir, &info, None, None).await;
        mock_gguf.assert_async().await;

        assert!(result.is_err(), "Download should fail for invalid hash");

        let path = app_dir.join("models").join("test-model-invalid.gguf");
        assert!(!path.exists(), "Failed download should clean up file");

        fs::remove_dir_all(&app_dir).await.ok();
    }
}
