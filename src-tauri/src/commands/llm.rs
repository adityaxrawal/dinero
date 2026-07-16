use crate::llm_manager::{self, LlmModelInfo};
use anyhow::Result;
use tauri::Manager;

#[tauri::command]
pub async fn llm_get_available_models() -> Result<Vec<LlmModelInfo>, crate::error::AppError> {
    Ok(llm_manager::get_available_models())
}

#[tauri::command]
pub async fn llm_download_model(
    app: tauri::AppHandle,
    model_id: String,
) -> Result<(), crate::error::AppError> {
    let app_dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));

    let models = llm_manager::get_available_models();
    let model_info = models
        .into_iter()
        .find(|m| m.id == model_id)
        .ok_or_else(|| crate::error::AppError::Validation("Model not found".to_string()))?;

    llm_manager::download_model(&app_dir, &model_info)
        .await
        .map_err(|e| crate::error::AppError::Network(e.to_string()))
}
