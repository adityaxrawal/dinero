use anyhow::Result;
use std::path::PathBuf;
use tokio::sync::{mpsc, oneshot};

pub struct LlmRequest {
    pub model_id: String,
    pub prompt: String,
    pub schema: Option<serde_json::Value>,
    pub ctx: crate::logging::llm_logger::LlmCallContext,
    pub app_dir: PathBuf,
    pub response_tx: oneshot::Sender<Result<String>>,
}

#[derive(Clone)]
pub struct LlmPipeline {
    tx: mpsc::Sender<LlmRequest>,
}

impl Default for LlmPipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl LlmPipeline {
    pub fn new() -> Self {
        let (tx, mut rx) = mpsc::channel::<LlmRequest>(1024);

        tauri::async_runtime::spawn(async move {
            while let Some(req) = rx.recv().await {
                tauri::async_runtime::spawn(async move {
                    // llama_sidecar enforces its own concurrency limit using a Semaphore
                    // calibrated to the machine's capabilities. We rely on that semaphore
                    // instead of building a duplicate one here.
                    let res = crate::llama_sidecar::complete_with_optional_schema_and_context(
                        &req.app_dir,
                        &req.model_id,
                        &req.prompt,
                        req.schema,
                        req.ctx,
                    )
                    .await;
                    let _ = req.response_tx.send(res);
                });
            }
        });

        Self { tx }
    }

    pub async fn enqueue(&self, req: LlmRequest) -> Result<()> {
        self.tx
            .send(req)
            .await
            .map_err(|_| anyhow::anyhow!("LLM Pipeline closed"))
    }
}
